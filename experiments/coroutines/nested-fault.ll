; E2.1: experiments/coroutines/fault.tellur lowered by hand (see cancellation.ll for
; the lowering rules). `fault` records the fault in the runtime and suspends; the
; driver then settles the owning domain: pending operations stop without events
; and every frame, suspended or completed, is destroyed (children first). A frame
; destroyed at a non-final suspension runs the same cleanup path as a completed one.

declare ptr @rt_frame_alloc(i64)
declare void @rt_frame_free(ptr)
declare void @rt_yield(i32)

declare void @rt_init(ptr)
declare i32 @rt_finish(i32)
declare i32 @rt_task_new(i32, i32)
declare void @rt_task_start(i32, ptr)
declare void @rt_complete(i32, i32, ptr)
declare i32 @rt_join(i32, i32)
declare i32 @rt_tick_start(i32, ptr, i32)
declare i32 @rt_op_poll(i32)
declare ptr @rt_op_payload(i32)
declare void @rt_fault(i32, ptr)
declare i32 @rt_faulted()
declare void @rt_emit(ptr)
declare void @rt_run(i32)

@one = private constant [4 x i8] c"one\00"
@failing_text = private constant [8 x i8] c"failing\00"
@message = private constant [15 x i8] c"sibling failed\00"

; async control step(string label) exits done
define ptr @step(i32 %task, ptr %label) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %s0 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s0, label %return [i8 0, label %start
                                i8 1, label %cleanup]
start:                                      ; handoff, then delta await tick(..)
  call void @rt_yield(i32 %task)
  %sh = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %sh, label %return [i8 0, label %begin
                                i8 1, label %cleanup]
begin:
  %r0 = call i32 @rt_tick_start(i32 %task, ptr %label, i32 0)
  br label %wait
wait:
  %r = phi i32 [ %r0, %begin ], [ %r1, %poll ]
  %pending = icmp eq i32 %r, -1
  br i1 %pending, label %suspend, label %ticked
suspend:
  %s1 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s1, label %return [i8 0, label %poll
                                i8 1, label %cleanup]
poll:
  %r1 = call i32 @rt_op_poll(i32 %task)
  br label %wait
ticked:                                     ; [ticked]: emit(result); exit done;
  %result = call ptr @rt_op_payload(i32 %task)
  call void @rt_emit(ptr %result)
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

; async control failing() exits done faults
define ptr @failure_body(i32 %task) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  br label %start
start:
  call void @rt_emit(ptr @failing_text)
  br label %yield
yield:                                      ; yield; yield; yield;
  %count = phi i32 [ 0, %start ], [ %next, %yielded ]
  %more = icmp ult i32 %count, 3
  br i1 %more, label %suspend, label %fault
suspend:
  call void @rt_yield(i32 %task)
  %s1 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s1, label %return [i8 0, label %yielded
                                i8 1, label %cleanup]
yielded:
  %next = add i32 %count, 1
  br label %yield
fault:                                      ; fault "sibling failed";
  call void @rt_fault(i32 %task, ptr @message)
  %sf = call i8 @llvm.coro.suspend(token none, i1 false)
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

; A by-handle direct-await child, explicitly destroyed during fault settlement.
define ptr @failing(i32 %task) presplitcoroutine {
entry:
  %child_slot = alloca ptr
  store ptr null, ptr %child_slot
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %s0 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s0, label %return [i8 0, label %start
                                i8 1, label %cleanup]
start:
  ; A direct await hands off before entering its child.
  call void @rt_yield(i32 %task)
  %sh = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %sh, label %return [i8 0, label %begin_child
                                i8 1, label %cleanup]
begin_child:
  %child = call ptr @failure_body(i32 %task)
  store ptr %child, ptr %child_slot
  br label %wait
wait:
  %s = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s, label %return [i8 0, label %resume
                               i8 1, label %cleanup]
resume:
  call void @llvm.coro.resume(ptr %child)
  br label %wait
cleanup:
  %active = load ptr, ptr %child_slot
  %live = icmp ne ptr %active, null
  br i1 %live, label %destroy_child, label %retire
destroy_child:
  call void @llvm.coro.destroy(ptr %active)
  br label %retire
retire:
  %m = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @rt_frame_free(ptr %m)
  br label %return
return:
  call void @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

; async control startup() exits done faults
define ptr @startup(i32 %task) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %pending = call i32 @rt_task_new(i32 %task, i32 -1)
  %pending_frame = call ptr @step(i32 %pending, ptr @one)
  call void @rt_task_start(i32 %pending, ptr %pending_frame)
  %failure = call i32 @rt_task_new(i32 %task, i32 -1)
  %failure_frame = call ptr @failing(i32 %failure)
  call void @rt_task_start(i32 %failure, ptr %failure_frame)
  br label %join_pending
join_pending:                               ; await pending;
  %j1 = call i32 @rt_join(i32 %task, i32 %pending)
  %p1 = icmp eq i32 %j1, -1
  br i1 %p1, label %wait_pending, label %join_failure
wait_pending:
  %s1 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s1, label %return [i8 0, label %join_pending
                                i8 1, label %cleanup]
join_failure:                               ; await failure;
  %j2 = call i32 @rt_join(i32 %task, i32 %failure)
  %p2 = icmp eq i32 %j2, -1
  br i1 %p2, label %wait_failure, label %done
wait_failure:
  %s2 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s2, label %return [i8 0, label %join_failure
                                i8 1, label %cleanup]
done:                                       ; exit done;
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

; control main() exits done faults { forward run startup(); }
define i32 @main(i32 %argc, ptr %argv) {
entry:
  %script_slot = getelementptr ptr, ptr %argv, i64 1
  %script = load ptr, ptr %script_slot
  call void @rt_init(ptr %script)
  %root = call i32 @rt_task_new(i32 -1, i32 -1)
  %frame = call ptr @startup(i32 %root)
  call void @rt_task_start(i32 %root, ptr %frame)
  call void @rt_run(i32 %root)
  %faulted = call i32 @rt_faulted()
  %settled = icmp ne i32 %faulted, 0
  br i1 %settled, label %report, label %completed
completed:
  call void @llvm.coro.destroy(ptr %frame)
  br label %report
report:
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

declare void @llvm.coro.resume(ptr)
