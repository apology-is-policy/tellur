; E2.1: playground/cancellation.tellur lowered by hand to LLVM switched-resume
; coroutines (llvm.coro.id / begin / suspend / free / end). Pinned: LLVM 22.1.x.
;
; Lowering rules used here:
; - An async control is a coroutine. Its parameters and every value live across a
;   suspension are spilled to the heap frame by CoroSplit; frames never move, so a
;   borrowed resource id or pointer stays valid while the frame is suspended.
; - Path labels are basic blocks; `delta` is a switch on the outcome code; a join
;   (`await child`) is a loop that suspends while the runtime reports PENDING (-1).
; - A spawned child starts suspended (initial suspend) and runs on its next turn.
; - Completion records the outcome in the runtime, then reaches the final suspend.
;   The frame is destroyed only by the consuming join (runtime) or, for a direct
;   await, by the awaiting frame after `llvm.coro.done`.
; - A direct await of an async control (`await first_answer(..)`) drives the
;   nested frame inside the caller's task: resume it, suspend while it is not done.
;
; Outcome codes: reader chunk=0 ended=1 stopped=2; first_answer answered=0
; unanswered=1; fetch fetched=0 unanswered=1 cleanup_failed=2; host receive data=0
; closed=1 failed=2 cancelled=99; open opened>=0; close closed=0 failed=1.

declare ptr @rt_frame_alloc(i64)
declare void @rt_frame_free(ptr)
declare void @rt_yield(i32)

declare void @rt_init(ptr)
declare i32 @rt_finish(i32)
declare i32 @rt_task_new(i32, i32)
declare void @rt_task_start(i32, ptr)
declare void @rt_cancel(i32)
declare void @rt_complete(i32, i32, ptr)
declare i32 @rt_join(i32, i32)
declare ptr @rt_payload(i32)
declare i32 @rt_receive_start(i32, i32, i32)
declare i32 @rt_op_poll(i32)
declare ptr @rt_op_payload(i32)
declare i32 @rt_open()
declare i32 @rt_close(i32)
declare i32 @rt_decode(ptr)
declare ptr @rt_text()
declare void @rt_emit(ptr)
declare void @rt_run(i32)

@late = private constant [58 x i8] c"The backup answered before its cancellation was observed.\00"
@stopped = private constant [20 x i8] c"The backup stopped.\00"
@closing = private constant [25 x i8] c"Closing a handle failed.\00"

; async control reader(ref handle input) exits chunk { bytes data }, ended, stopped
define ptr @reader(i32 %task, i32 %input) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  ; spawned: the body runs on the task's next turn
  %s0 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s0, label %return [i8 0, label %start
                                i8 1, label %cleanup]
start:
  ; an await of a host operation is a handoff: requeue, then start on the next turn
  call void @rt_yield(i32 %task)
  %sh = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %sh, label %return [i8 0, label %begin
                                i8 1, label %cleanup]
begin:
  ; delta await receive(input) { ..., cancelled => cancelled }
  %r0 = call i32 @rt_receive_start(i32 %task, i32 %input, i32 1)
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
  switch i32 %r, label %ended [i32 0, label %received
                               i32 99, label %cancelled]
received:                                   ; [received]: exit chunk { data };
  %data = call ptr @rt_op_payload(i32 %task)
  call void @rt_complete(i32 %task, i32 0, ptr %data)
  br label %final
ended:                                      ; [ended | failed]: exit ended;
  call void @rt_complete(i32 %task, i32 1, ptr null)
  br label %final
cancelled:                                  ; [cancelled]: exit stopped;
  call void @rt_complete(i32 %task, i32 2, ptr null)
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

; async control first_answer(ref handle primary, ref handle backup)
;     exits answered { bytes data }, unanswered
define ptr @first_answer(i32 %task, i32 %primary, i32 %backup, ptr %out, ptr %out_data) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  ; auto first = spawn reader(primary); auto second = spawn reader(backup);
  ; each child borrows its handle until it is joined
  %first = call i32 @rt_task_new(i32 %task, i32 %primary)
  %first_frame = call ptr @reader(i32 %first, i32 %primary)
  call void @rt_task_start(i32 %first, ptr %first_frame)
  %second = call i32 @rt_task_new(i32 %task, i32 %backup)
  %second_frame = call ptr @reader(i32 %second, i32 %backup)
  call void @rt_task_start(i32 %second, ptr %second_frame)
  br label %join_first
join_first:                                 ; delta await first { .. }
  %j1 = call i32 @rt_join(i32 %task, i32 %first)
  %p1 = icmp eq i32 %j1, -1
  br i1 %p1, label %wait_first, label %first_joined
wait_first:
  %s1 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s1, label %return [i8 0, label %join_first
                                i8 1, label %cleanup]
first_joined:
  switch i32 %j1, label %primary_ended [i32 0, label %primary_answered]
primary_answered:                                    ; [primary]: cancel second; delta await second
  %data = call ptr @rt_payload(i32 %first)
  call void @rt_cancel(i32 %second)
  br label %join_second
join_second:
  %j2 = call i32 @rt_join(i32 %task, i32 %second)
  %p2 = icmp eq i32 %j2, -1
  br i1 %p2, label %wait_second, label %second_joined
wait_second:
  %s2 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s2, label %return [i8 0, label %join_second
                                i8 1, label %cleanup]
second_joined:
  switch i32 %j2, label %answered_block [i32 0, label %late
                                   i32 2, label %backup_stopped]
late:
  call void @rt_emit(ptr @late)
  br label %answered_block
backup_stopped:
  call void @rt_emit(ptr @stopped)
  br label %answered_block
answered_block:                                   ; [late | backup_ended | backup_stopped]
  ; `data` was produced before two suspensions; it lives in the frame
  store i32 0, ptr %out
  store ptr %data, ptr %out_data
  br label %final
primary_ended:                              ; [primary_ended | primary_stopped]
  %j3 = call i32 @rt_join(i32 %task, i32 %second)
  %p3 = icmp eq i32 %j3, -1
  br i1 %p3, label %wait_backup, label %backup_joined
wait_backup:
  %s3 = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s3, label %return [i8 0, label %primary_ended
                                i8 1, label %cleanup]
backup_joined:
  %answer = icmp eq i32 %j3, 0
  br i1 %answer, label %backup_answered, label %unanswered
backup_answered:
  %backup_data = call ptr @rt_payload(i32 %second)
  store i32 0, ptr %out
  store ptr %backup_data, ptr %out_data
  br label %final
unanswered:
  store i32 1, ptr %out
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

; async control fetch(handle primary, handle backup)
;     exits fetched { bytes data }, unanswered, cleanup_failed { pair_close_error error }
define ptr @fetch(i32 %task, i32 %primary, i32 %backup, ptr %out, ptr %out_data) presplitcoroutine {
entry:
  %answer = alloca i32
  %answer_data = alloca ptr
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @rt_frame_alloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  ; auto answer = capture await first_answer(borrow primary, borrow backup);
  call void @rt_yield(i32 %task)
  %sh = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %sh, label %return [i8 0, label %begin_child
                                i8 1, label %cleanup]
begin_child:
  %inner = call ptr @first_answer(i32 %task, i32 %primary, i32 %backup, ptr %answer, ptr %answer_data)
  br label %drive
drive:
  %done = call i1 @llvm.coro.done(ptr %inner)
  br i1 %done, label %awaited, label %wait
wait:
  %s = call i8 @llvm.coro.suspend(token none, i1 false)
  switch i8 %s, label %return [i8 0, label %resume_inner
                               i8 1, label %cleanup]
resume_inner:
  call void @llvm.coro.resume(ptr %inner)
  br label %drive
awaited:
  ; the direct await consumed the nested frame: destroy it after completion
  call void @llvm.coro.destroy(ptr %inner)
  ; both closes are attempted after the join ended the loans
  %primary_closed = call i32 @rt_close(i32 %primary)
  %backup_closed = call i32 @rt_close(i32 %backup)
  %closes = or i32 %primary_closed, %backup_closed
  %close_failed = icmp ne i32 %closes, 0
  br i1 %close_failed, label %cleanup_failed, label %closed_block
cleanup_failed:
  call void @rt_complete(i32 %task, i32 2, ptr null)
  store i32 2, ptr %out
  br label %final
closed_block:
  %a = load i32, ptr %answer
  %answered = icmp eq i32 %a, 0
  br i1 %answered, label %fetched_block, label %unanswered_block
fetched_block:
  %d = load ptr, ptr %answer_data
  store i32 0, ptr %out
  store ptr %d, ptr %out_data
  call void @rt_complete(i32 %task, i32 0, ptr null)
  br label %final
unanswered_block:
  store i32 1, ptr %out
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

; control main() exits process_result (answered 0, unanswered 3, unavailable 2,
; cleanup_failed 1). `run fetch(..)` drives the root task to completion.
define i32 @main(i32 %argc, ptr %argv) {
entry:
  %out = alloca i32
  %out_data = alloca ptr
  %script_slot = getelementptr ptr, ptr %argv, i64 1
  %script = load ptr, ptr %script_slot
  call void @rt_init(ptr %script)
  %primary = call i32 @rt_open()
  %primary_open = icmp sge i32 %primary, 0
  br i1 %primary_open, label %open_backup, label %unavailable
open_backup:
  %backup = call i32 @rt_open()
  %both = icmp sge i32 %backup, 0
  br i1 %both, label %both_open, label %backup_missing
backup_missing:
  %alone = call i32 @rt_close(i32 %primary)
  %alone_failed = icmp ne i32 %alone, 0
  br i1 %alone_failed, label %cleanup_failed, label %unavailable
both_open:
  %root = call i32 @rt_task_new(i32 -1, i32 -1)
  %frame = call ptr @fetch(i32 %root, i32 %primary, i32 %backup, ptr %out, ptr %out_data)
  call void @rt_task_start(i32 %root, ptr %frame)
  call void @rt_run(i32 %root)
  call void @llvm.coro.destroy(ptr %frame)
  %o = load i32, ptr %out
  switch i32 %o, label %cleanup_failed [i32 0, label %fetched
                                        i32 1, label %unanswered]
fetched:
  %data = load ptr, ptr %out_data
  %decoded = call i32 @rt_decode(ptr %data)
  %valid = icmp eq i32 %decoded, 0
  br i1 %valid, label %answered, label %unanswered
answered:
  %text = call ptr @rt_text()
  call void @rt_emit(ptr %text)
  br label %exit
unanswered:
  br label %exit
unavailable:
  br label %exit
cleanup_failed:
  call void @rt_emit(ptr @closing)
  br label %exit
exit:
  %status = phi i32 [ 0, %answered ], [ 3, %unanswered ], [ 2, %unavailable ], [ 1, %cleanup_failed ]
  %result = call i32 @rt_finish(i32 %status)
  ret i32 %result
}

declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare void @llvm.coro.end(ptr, i1, token)
declare i1 @llvm.coro.done(ptr)
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
