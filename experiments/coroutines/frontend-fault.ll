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
declare i32 @rt_join(i32, i32)
declare i32 @rt_tick_start(i32, ptr, i32)
declare i32 @rt_op_poll(i32)
declare ptr @rt_op_payload(i32)
declare void @rt_fault(i32, ptr)
declare i32 @rt_faulted()
declare void @rt_emit(ptr)

@one = private constant [4 x i8] c"one\00"
@failing_text = private constant [8 x i8] c"failing\00"
@message = private constant [15 x i8] c"sibling failed\00"

%step = type { %Frame, i32, ptr }
define ptr @step_init(i32 %task, ptr %label) {
entry:
  %end = getelementptr %step, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @step_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @step_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %step, ptr %f, i32 0, i32 1
  %label_slot = getelementptr %step, ptr %f, i32 0, i32 2
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store ptr %label, ptr %label_slot
  ret ptr %f
}

define void @step_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %step, ptr %f, i32 0, i32 1
  %label_slot = getelementptr %step, ptr %f, i32 0, i32 2
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
  %label = load ptr, ptr %label_slot
  %r0 = call i32 @rt_tick_start(i32 %task, ptr %label, i32 0)
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
  %result = call ptr @rt_op_payload(i32 %owner)
  call void @rt_emit(ptr %result)
  call void @rt_complete(i32 %owner, i32 0, ptr null)
  br label %final
final:
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @step_destroy(ptr %f) {
entry:
  call void @rt_frame_free(ptr %f)
  ret void
}

%failing = type { %Frame, i32, i32 }
define ptr @failing_init(i32 %task) {
entry:
  %end = getelementptr %failing, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @failing_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @failing_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %failing, ptr %f, i32 0, i32 1
  %count_slot = getelementptr %failing, ptr %f, i32 0, i32 2
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store i32 0, ptr %count_slot
  ret ptr %f
}

define void @failing_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %failing, ptr %f, i32 0, i32 1
  %count_slot = getelementptr %failing, ptr %f, i32 0, i32 2
  %state = load i32, ptr %state_slot
  switch i32 %state, label %invalid [
    i32 0, label %start
    i32 1, label %yielded
  ]
start:
  call void @rt_emit(ptr @failing_text)
  br label %yield
yielded:
  %count0 = load i32, ptr %count_slot
  %next = add i32 %count0, 1
  store i32 %next, ptr %count_slot
  br label %yield
yield:
  %task = load i32, ptr %task_slot
  %count = load i32, ptr %count_slot
  %more = icmp ult i32 %count, 3
  br i1 %more, label %suspend, label %fault
suspend:
  store i32 1, ptr %state_slot
  call void @rt_yield(i32 %task)
  ret void
fault:
  store i32 -1, ptr %state_slot
  call void @rt_fault(i32 %task, ptr @message)
  ret void
invalid:
  unreachable
}

define void @failing_destroy(ptr %f) {
entry:
  call void @rt_frame_free(ptr %f)
  ret void
}

%startup = type { %Frame, i32, i32, i32 }
define ptr @startup_init(i32 %task) {
entry:
  %end = getelementptr %startup, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @startup_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @startup_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %startup, ptr %f, i32 0, i32 1
  %pending_slot = getelementptr %startup, ptr %f, i32 0, i32 2
  %failure_slot = getelementptr %startup, ptr %f, i32 0, i32 3
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  call void @startup_resume(ptr %f)
  ret ptr %f
}

define void @startup_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %startup, ptr %f, i32 0, i32 1
  %pending_slot = getelementptr %startup, ptr %f, i32 0, i32 2
  %failure_slot = getelementptr %startup, ptr %f, i32 0, i32 3
  %state = load i32, ptr %state_slot
  switch i32 %state, label %invalid [
    i32 0, label %start
    i32 1, label %join_pending
    i32 2, label %join_failure
  ]
start:
  %task0 = load i32, ptr %task_slot
  %pending0 = call i32 @rt_task_new(i32 %task0, i32 -1)
  %pending_frame = call ptr @step_init(i32 %pending0, ptr @one)
  call void @rt_task_start(i32 %pending0, ptr %pending_frame)
  store i32 %pending0, ptr %pending_slot
  %failure0 = call i32 @rt_task_new(i32 %task0, i32 -1)
  %failure_frame = call ptr @failing_init(i32 %failure0)
  call void @rt_task_start(i32 %failure0, ptr %failure_frame)
  store i32 %failure0, ptr %failure_slot
  br label %join_pending
join_pending:
  %task1 = load i32, ptr %task_slot
  %pending = load i32, ptr %pending_slot
  %j1 = call i32 @rt_join(i32 %task1, i32 %pending)
  %p1 = icmp eq i32 %j1, -1
  br i1 %p1, label %wait_pending, label %join_failure
wait_pending:
  store i32 1, ptr %state_slot
  ret void
join_failure:
  %task2 = load i32, ptr %task_slot
  %failure = load i32, ptr %failure_slot
  %j2 = call i32 @rt_join(i32 %task2, i32 %failure)
  %p2 = icmp eq i32 %j2, -1
  br i1 %p2, label %wait_failure, label %done
wait_failure:
  store i32 2, ptr %state_slot
  ret void
done:
  call void @rt_complete(i32 %task2, i32 0, ptr null)
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @startup_destroy(ptr %f) {
entry:
  call void @rt_frame_free(ptr %f)
  ret void
}

define i32 @main(i32 %argc, ptr %argv) {
entry:
  %script_slot = getelementptr ptr, ptr %argv, i64 1
  %script = load ptr, ptr %script_slot
  call void @rt_init(ptr %script)
  %root = call i32 @rt_task_new(i32 -1, i32 -1)
  %frame = call ptr @startup_init(i32 %root)
  call void @rt_task_start(i32 %root, ptr %frame)
  call void @rt_run(i32 %root)
  %faulted = call i32 @rt_faulted()
  %settled = icmp ne i32 %faulted, 0
  br i1 %settled, label %report, label %completed
completed:
  call void @coro_destroy(ptr %frame)
  br label %report
report:
  %result = call i32 @rt_finish(i32 0)
  ret i32 %result
}
