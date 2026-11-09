; E2.1: experiments/coroutines/sets.tellur lowered by hand (see cancellation.ll for
; the lowering rules). A task set is runtime state; `await next` is a loop that
; suspends while no member has completed, then joins the earliest completed one.
;
; Outcome codes: worker done=0 stopped=1; tick ticked=0 cancelled=99;
; rt_set_next member id, -2 empty, -1 pending.

declare ptr @rt_frame_alloc(i64)
declare void @rt_frame_free(ptr)
declare void @rt_yield(i32)

declare void @rt_init(ptr)
declare i32 @rt_finish(i32)
declare i32 @rt_task_new(i32, i32)
declare void @rt_task_start(i32, ptr)
declare void @rt_complete(i32, i32, ptr)
declare ptr @rt_payload(i32)
declare i32 @rt_outcome(i32)
declare i32 @rt_tick_start(i32, ptr, i32)
declare i32 @rt_op_poll(i32)
declare ptr @rt_op_payload(i32)
declare i32 @rt_set_new()
declare void @rt_set_add(i32, i32)
declare void @rt_set_cancel(i32)
declare i32 @rt_set_next(i32, i32)
declare void @rt_emit(ptr)
declare void @rt_run(i32)

@a = private constant [2 x i8] c"a\00"
@b = private constant [2 x i8] c"b\00"
@c = private constant [2 x i8] c"c\00"
@stopped = private constant [8 x i8] c"stopped\00"

; async control worker(string label) exits done { string label }, stopped
define ptr @worker(i32 %task, ptr %label) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %s0 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s0, label %return [i8 0, label %start
                                i8 1, label %cleanup]
start:                                      ; delta await tick(move label) { .. }
  ; an await of a host operation is a handoff: requeue, then start on the next turn
  call void @rt_yield(i32 %task)
  %sh = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %sh, label %return [i8 0, label %begin
                                i8 1, label %cleanup]
begin:
  %r0 = call i32 @rt_tick_start(i32 %task, ptr %label, i32 1)
  br label %wait
wait:
  %r = phi i32 [ %r0, %begin ], [ %r1, %poll ]
  %pending = icmp eq i32 %r, -1
  br i1 %pending, label %suspend, label %decided
suspend:
  %s1 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s1, label %return [i8 0, label %poll
                                i8 1, label %cleanup]
poll:
  %r1 = call i32 @rt_op_poll(i32 %task)
  br label %wait
decided:
  %was_cancelled = icmp eq i32 %r, 99
  br i1 %was_cancelled, label %cancelled, label %ticked
ticked:                                     ; [ticked]: exit done { result };
  %result = call ptr @rt_op_payload(i32 %task)
  call void @rt_complete(i32 %task, i32 0, ptr %result)
  br label %final
cancelled:                                  ; [cancelled]: exit stopped;
  call void @rt_complete(i32 %task, i32 1, ptr null)
  br label %final
final:
  %sf = call i8 @llvm.coro.suspend(token none, i1 true)
  switch i8 %sf, label %return [i8 0, label %unreachable
                                i8 1, label %cleanup]
unreachable:
  unreachable
cleanup:
  %m = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @rt_frame_free(ptr %m)
  br label %return
return:
  call void @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

; spawn worker(label) into members
define void @spawn_member(i32 %owner, i32 %set, ptr %label) {
  %member = call i32 @rt_task_new(i32 %owner, i32 -1)
  %frame = call ptr @worker(i32 %member, ptr %label)
  call void @rt_task_start(i32 %member, ptr %frame)
  call void @rt_set_add(i32 %set, i32 %member)
  ret void
}

; async control gather() exits done
define ptr @gather(i32 %task) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %members = call i32 @rt_set_new()
  call void @spawn_member(i32 %task, i32 %members, ptr @a)
  call void @spawn_member(i32 %task, i32 %members, ptr @b)
  call void @spawn_member(i32 %task, i32 %members, ptr @c)
  br label %drain
drain:                                      ; loop drain(bool first = true, ..)
  %first = phi i1 [ true, %entry ], [ false, %again ]
  br label %next
next:                                       ; delta await next active { .. }
  %member = call i32 @rt_set_next(i32 %task, i32 %members)
  %pending = icmp eq i32 %member, -1
  br i1 %pending, label %wait, label %joined
wait:
  %s = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s, label %return [i8 0, label %next
                               i8 1, label %cleanup]
joined:
  %empty = icmp eq i32 %member, -2
  br i1 %empty, label %drained, label %member_joined
member_joined:
  %outcome = call i32 @rt_outcome(i32 %member)
  %done = icmp eq i32 %outcome, 0
  br i1 %done, label %finished, label %halted
finished:                                   ; [finished]: emit(label);
  %label = call ptr @rt_payload(i32 %member)
  call void @rt_emit(ptr %label)
  br label %decide
halted:                                     ; [halted]: emit("stopped");
  call void @rt_emit(ptr @stopped)
  br label %decide
decide:                                     ; delta first { true => winner, .. }
  br i1 %first, label %winner, label %again
winner:                                     ; cancel active;
  call void @rt_set_cancel(i32 %members)
  br label %again
again:
  br label %drain
drained:                                    ; [drained]: exit done;
  call void @rt_complete(i32 %task, i32 0, ptr null)
  %sf = call i8 @llvm.coro.suspend(token none, i1 true)
  switch i8 %sf, label %return [i8 0, label %unreachable
                                i8 1, label %cleanup]
unreachable:
  unreachable
cleanup:
  %m = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @rt_frame_free(ptr %m)
  br label %return
return:
  call void @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

; control main() exits process_result { forward run gather(); }
define i32 @main(i32 %argc, ptr %argv) {
entry:
  %script_slot = getelementptr ptr, ptr %argv, i64 1
  %script = load ptr, ptr %script_slot
  call void @rt_init(ptr %script)
  %root = call i32 @rt_task_new(i32 -1, i32 -1)
  %frame = call ptr @gather(i32 %root)
  call void @rt_task_start(i32 %root, ptr %frame)
  call void @rt_run(i32 %root)
  call void @llvm.coro.destroy(ptr %frame)
  %result = call i32 @rt_finish(i32 0)
  ret i32 %result
}

declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare void @llvm.coro.end(ptr, i1, token)
declare void @llvm.coro.destroy(ptr)
