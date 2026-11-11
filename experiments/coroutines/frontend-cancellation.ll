; B1.4a: hand-lowered frontend state machines. No LLVM coroutine intrinsics.
; All values live over suspension are explicit fields; direct children are by value.
%Frame = type { ptr, ptr, i32, i32 }
declare ptr @rt_arena_alloc(i64)
declare void @rt_frame_free(ptr)
declare void @rt_inline_enter(ptr)
declare void @rt_inline_leave(ptr)
declare void @coro_destroy(ptr)
declare void @rt_init(ptr)
declare i32 @rt_finish(i32)
declare i32 @rt_task_new(i32, i32)
declare void @rt_task_start(i32, ptr)
declare void @rt_yield(i32)
declare void @rt_complete(i32, i32, ptr)
declare void @rt_run(i32)
declare void @rt_cancel(i32)
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

@late = private constant [58 x i8] c"The backup answered before its cancellation was observed.\00"
@stopped = private constant [20 x i8] c"The backup stopped.\00"
@closing = private constant [25 x i8] c"Closing a handle failed.\00"

%reader = type { %Frame, i32, i32 }
define ptr @reader_init(i32 %task, i32 %input) {
entry:
  %end = getelementptr %reader, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @reader_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @reader_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %reader, ptr %f, i32 0, i32 1
  %input_slot = getelementptr %reader, ptr %f, i32 0, i32 2
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store i32 %input, ptr %input_slot
  ret ptr %f
}

define void @reader_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %reader, ptr %f, i32 0, i32 1
  %input_slot = getelementptr %reader, ptr %f, i32 0, i32 2
  %state = load i32, ptr %state_slot
  switch i32 %state, label %invalid [
    i32 0, label %start
    i32 1, label %begin
    i32 2, label %poll
  ]
start:
  %task0 = load i32, ptr %task_slot
  store i32 1, ptr %state_slot
  call void @rt_yield(i32 %task0)
  ret void
begin:
  %task = load i32, ptr %task_slot
  %input = load i32, ptr %input_slot
  %r0 = call i32 @rt_receive_start(i32 %task, i32 %input, i32 1)
  br label %wait
poll:
  %task1 = load i32, ptr %task_slot
  %r1 = call i32 @rt_op_poll(i32 %task1)
  br label %wait
wait:
  %r = phi i32 [ %r0, %begin ], [ %r1, %poll ]
  %pending = icmp eq i32 %r, -1
  br i1 %pending, label %suspend, label %decided
suspend:
  store i32 2, ptr %state_slot
  ret void
decided:
  %owner = load i32, ptr %task_slot
  switch i32 %r, label %ended [ i32 0, label %received
                                          i32 99, label %cancelled ]
received:
  %data = call ptr @rt_op_payload(i32 %owner)
  call void @rt_complete(i32 %owner, i32 0, ptr %data)
  br label %final
ended:
  call void @rt_complete(i32 %owner, i32 1, ptr null)
  br label %final
cancelled:
  call void @rt_complete(i32 %owner, i32 2, ptr null)
  br label %final
final:
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @reader_destroy(ptr %f) {
entry:
  call void @rt_frame_free(ptr %f)
  ret void
}

%answer = type { %Frame, i32, i32, i32, i32, i32, i32, ptr }
define void @answer_init(ptr %f, i32 %task, i32 %primary, i32 %backup) {
entry:
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @answer_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @answer_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %answer, ptr %f, i32 0, i32 1
  %primary_slot = getelementptr %answer, ptr %f, i32 0, i32 2
  %backup_slot = getelementptr %answer, ptr %f, i32 0, i32 3
  %first_slot = getelementptr %answer, ptr %f, i32 0, i32 4
  %second_slot = getelementptr %answer, ptr %f, i32 0, i32 5
  %outcome_slot = getelementptr %answer, ptr %f, i32 0, i32 6
  %data_slot = getelementptr %answer, ptr %f, i32 0, i32 7
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store i32 %primary, ptr %primary_slot
  store i32 %backup, ptr %backup_slot
  call void @rt_inline_enter(ptr %f)
  call void @answer_resume(ptr %f)
  ret void
}

define void @answer_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %answer, ptr %f, i32 0, i32 1
  %primary_slot = getelementptr %answer, ptr %f, i32 0, i32 2
  %backup_slot = getelementptr %answer, ptr %f, i32 0, i32 3
  %first_slot = getelementptr %answer, ptr %f, i32 0, i32 4
  %second_slot = getelementptr %answer, ptr %f, i32 0, i32 5
  %outcome_slot = getelementptr %answer, ptr %f, i32 0, i32 6
  %data_slot = getelementptr %answer, ptr %f, i32 0, i32 7
  %state = load i32, ptr %state_slot
  switch i32 %state, label %invalid [
    i32 0, label %start
    i32 1, label %join_first
    i32 2, label %join_second
    i32 3, label %join_backup
  ]
start:
  %task0 = load i32, ptr %task_slot
  %primary = load i32, ptr %primary_slot
  %backup = load i32, ptr %backup_slot
  %first0 = call i32 @rt_task_new(i32 %task0, i32 %primary)
  %first_frame = call ptr @reader_init(i32 %first0, i32 %primary)
  call void @rt_task_start(i32 %first0, ptr %first_frame)
  store i32 %first0, ptr %first_slot
  %second0 = call i32 @rt_task_new(i32 %task0, i32 %backup)
  %second_frame = call ptr @reader_init(i32 %second0, i32 %backup)
  call void @rt_task_start(i32 %second0, ptr %second_frame)
  store i32 %second0, ptr %second_slot
  br label %join_first
join_first:
  %task1 = load i32, ptr %task_slot
  %first = load i32, ptr %first_slot
  %j1 = call i32 @rt_join(i32 %task1, i32 %first)
  %p1 = icmp eq i32 %j1, -1
  br i1 %p1, label %wait_first, label %first_joined
wait_first:
  store i32 1, ptr %state_slot
  ret void
first_joined:
  %got = icmp eq i32 %j1, 0
  br i1 %got, label %primary_answered, label %join_backup
primary_answered:
  %data = call ptr @rt_payload(i32 %first)
  store ptr %data, ptr %data_slot
  %second = load i32, ptr %second_slot
  call void @rt_cancel(i32 %second)
  br label %join_second
join_second:
  %task2 = load i32, ptr %task_slot
  %second2 = load i32, ptr %second_slot
  %j2 = call i32 @rt_join(i32 %task2, i32 %second2)
  %p2 = icmp eq i32 %j2, -1
  br i1 %p2, label %wait_second, label %second_joined
wait_second:
  store i32 2, ptr %state_slot
  ret void
second_joined:
  switch i32 %j2, label %answered [ i32 0, label %late
                                   i32 2, label %backup_stopped ]
late:
  call void @rt_emit(ptr @late)
  br label %answered
backup_stopped:
  call void @rt_emit(ptr @stopped)
  br label %answered
answered:
  store i32 0, ptr %outcome_slot
  br label %final
join_backup:
  %task3 = load i32, ptr %task_slot
  %second3 = load i32, ptr %second_slot
  %j3 = call i32 @rt_join(i32 %task3, i32 %second3)
  %p3 = icmp eq i32 %j3, -1
  br i1 %p3, label %wait_backup, label %backup_joined
wait_backup:
  store i32 3, ptr %state_slot
  ret void
backup_joined:
  %answer = icmp eq i32 %j3, 0
  br i1 %answer, label %backup_answered, label %unanswered
backup_answered:
  %backup_data = call ptr @rt_payload(i32 %second3)
  store ptr %backup_data, ptr %data_slot
  br label %answered
unanswered:
  store i32 1, ptr %outcome_slot
  br label %final
final:
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @answer_destroy(ptr %f) {
entry:
  call void @rt_inline_leave(ptr %f)
  ret void
}

%fetch = type { %Frame, i32, i32, i32, ptr, ptr, i32, %answer }
define ptr @fetch_init(i32 %task, i32 %primary, i32 %backup, ptr %out, ptr %out_data) {
entry:
  %end = getelementptr %fetch, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @fetch_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @fetch_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %fetch, ptr %f, i32 0, i32 1
  %primary_slot = getelementptr %fetch, ptr %f, i32 0, i32 2
  %backup_slot = getelementptr %fetch, ptr %f, i32 0, i32 3
  %out_slot = getelementptr %fetch, ptr %f, i32 0, i32 4
  %out_data_slot = getelementptr %fetch, ptr %f, i32 0, i32 5
  %child_live_slot = getelementptr %fetch, ptr %f, i32 0, i32 6
  %child_slot = getelementptr %fetch, ptr %f, i32 0, i32 7
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store i32 %primary, ptr %primary_slot
  store i32 %backup, ptr %backup_slot
  store ptr %out, ptr %out_slot
  store ptr %out_data, ptr %out_data_slot
  call void @fetch_resume(ptr %f)
  ret ptr %f
}

define void @fetch_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %fetch, ptr %f, i32 0, i32 1
  %primary_slot = getelementptr %fetch, ptr %f, i32 0, i32 2
  %backup_slot = getelementptr %fetch, ptr %f, i32 0, i32 3
  %out_slot = getelementptr %fetch, ptr %f, i32 0, i32 4
  %out_data_slot = getelementptr %fetch, ptr %f, i32 0, i32 5
  %child_live_slot = getelementptr %fetch, ptr %f, i32 0, i32 6
  %child_slot = getelementptr %fetch, ptr %f, i32 0, i32 7
  %state = load i32, ptr %state_slot
  switch i32 %state, label %invalid [
    i32 0, label %start
    i32 1, label %begin_child
    i32 2, label %resume_child
  ]
start:
  %owner = load i32, ptr %task_slot
  store i32 1, ptr %state_slot
  call void @rt_yield(i32 %owner)
  ret void
begin_child:
  %task0 = load i32, ptr %task_slot
  %primary0 = load i32, ptr %primary_slot
  %backup0 = load i32, ptr %backup_slot
  store i32 1, ptr %child_live_slot
  call void @answer_init(ptr %child_slot, i32 %task0, i32 %primary0, i32 %backup0)
  br label %drive
resume_child:
  call void @answer_resume(ptr %child_slot)
  br label %drive
drive:
  %child_done_slot = getelementptr %Frame, ptr %child_slot, i32 0, i32 3
  %done = load i32, ptr %child_done_slot
  %finished = icmp ne i32 %done, 0
  br i1 %finished, label %awaited, label %wait
wait:
  store i32 2, ptr %state_slot
  ret void
awaited:
  %answer_slot = getelementptr %answer, ptr %child_slot, i32 0, i32 6
  %answer_data_slot = getelementptr %answer, ptr %child_slot, i32 0, i32 7
  %a = load i32, ptr %answer_slot
  %d = load ptr, ptr %answer_data_slot
  call void @answer_destroy(ptr %child_slot)
  store i32 0, ptr %child_live_slot
  %primary = load i32, ptr %primary_slot
  %backup = load i32, ptr %backup_slot
  %primary_closed = call i32 @rt_close(i32 %primary)
  %backup_closed = call i32 @rt_close(i32 %backup)
  %closes = or i32 %primary_closed, %backup_closed
  %close_failed = icmp ne i32 %closes, 0
  %result = select i1 %close_failed, i32 2, i32 %a
  %out = load ptr, ptr %out_slot
  %out_data = load ptr, ptr %out_data_slot
  store i32 %result, ptr %out
  store ptr %d, ptr %out_data
  %task = load i32, ptr %task_slot
  call void @rt_complete(i32 %task, i32 %result, ptr null)
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @fetch_destroy(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %fetch, ptr %f, i32 0, i32 1
  %primary_slot = getelementptr %fetch, ptr %f, i32 0, i32 2
  %backup_slot = getelementptr %fetch, ptr %f, i32 0, i32 3
  %out_slot = getelementptr %fetch, ptr %f, i32 0, i32 4
  %out_data_slot = getelementptr %fetch, ptr %f, i32 0, i32 5
  %child_live_slot = getelementptr %fetch, ptr %f, i32 0, i32 6
  %child_slot = getelementptr %fetch, ptr %f, i32 0, i32 7
  %live = load i32, ptr %child_live_slot
  %active = icmp ne i32 %live, 0
  br i1 %active, label %child, label %retire
child:
  call void @answer_destroy(ptr %child_slot)
  store i32 0, ptr %child_live_slot
  br label %retire
retire:
  call void @rt_frame_free(ptr %f)
  ret void
}

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
  %frame = call ptr @fetch_init(i32 %root, i32 %primary, i32 %backup, ptr %out, ptr %out_data)
  call void @rt_task_start(i32 %root, ptr %frame)
  call void @rt_run(i32 %root)
  call void @coro_destroy(ptr %frame)
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
