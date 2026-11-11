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

@a = private constant [2 x i8] c"a\00"
@b = private constant [2 x i8] c"b\00"
@c = private constant [2 x i8] c"c\00"
@stopped = private constant [8 x i8] c"stopped\00"

%worker = type { %Frame, i32, ptr }
define ptr @worker_init(i32 %task, ptr %label) {
entry:
  %end = getelementptr %worker, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @worker_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @worker_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %worker, ptr %f, i32 0, i32 1
  %label_slot = getelementptr %worker, ptr %f, i32 0, i32 2
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store ptr %label, ptr %label_slot
  ret ptr %f
}

define void @worker_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %worker, ptr %f, i32 0, i32 1
  %label_slot = getelementptr %worker, ptr %f, i32 0, i32 2
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
  %r0 = call i32 @rt_tick_start(i32 %task, ptr %label, i32 1)
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
  %cancelled = icmp eq i32 %r, 99
  br i1 %cancelled, label %stopped, label %ticked
stopped:
  call void @rt_complete(i32 %owner, i32 1, ptr null)
  br label %final
ticked:
  %result = call ptr @rt_op_payload(i32 %owner)
  call void @rt_complete(i32 %owner, i32 0, ptr %result)
  br label %final
final:
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @worker_destroy(ptr %f) {
entry:
  call void @rt_frame_free(ptr %f)
  ret void
}

define void @spawn_member(i32 %owner, i32 %set, ptr %label) {
  %member = call i32 @rt_task_new(i32 %owner, i32 -1)
  %frame = call ptr @worker_init(i32 %member, ptr %label)
  call void @rt_task_start(i32 %member, ptr %frame)
  call void @rt_set_add(i32 %set, i32 %member)
  ret void
}

%gather = type { %Frame, i32, i32, i32 }
define ptr @gather_init(i32 %task) {
entry:
  %end = getelementptr %gather, ptr null, i32 1
  %size = ptrtoint ptr %end to i64
  %f = call ptr @rt_arena_alloc(i64 %size)
  %resume_slot = getelementptr %Frame, ptr %f, i32 0, i32 0
  store ptr @gather_resume, ptr %resume_slot
  %destroy_slot = getelementptr %Frame, ptr %f, i32 0, i32 1
  store ptr @gather_destroy, ptr %destroy_slot
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %gather, ptr %f, i32 0, i32 1
  %members_slot = getelementptr %gather, ptr %f, i32 0, i32 2
  %first_slot = getelementptr %gather, ptr %f, i32 0, i32 3
  store i32 0, ptr %state_slot
  store i32 0, ptr %done_slot
  store i32 %task, ptr %task_slot
  store i32 1, ptr %first_slot
  call void @gather_resume(ptr %f)
  ret ptr %f
}

define void @gather_resume(ptr %f) {
entry:
  %state_slot = getelementptr %Frame, ptr %f, i32 0, i32 2
  %done_slot = getelementptr %Frame, ptr %f, i32 0, i32 3
  %task_slot = getelementptr %gather, ptr %f, i32 0, i32 1
  %members_slot = getelementptr %gather, ptr %f, i32 0, i32 2
  %first_slot = getelementptr %gather, ptr %f, i32 0, i32 3
  %state = load i32, ptr %state_slot
  switch i32 %state, label %invalid [
    i32 0, label %start
    i32 1, label %next
  ]
start:
  %task0 = load i32, ptr %task_slot
  %members0 = call i32 @rt_set_new()
  store i32 %members0, ptr %members_slot
  call void @spawn_member(i32 %task0, i32 %members0, ptr @a)
  call void @spawn_member(i32 %task0, i32 %members0, ptr @b)
  call void @spawn_member(i32 %task0, i32 %members0, ptr @c)
  br label %next
next:
  %task = load i32, ptr %task_slot
  %members = load i32, ptr %members_slot
  %member = call i32 @rt_set_next(i32 %task, i32 %members)
  %pending = icmp eq i32 %member, -1
  br i1 %pending, label %wait, label %joined
wait:
  store i32 1, ptr %state_slot
  ret void
joined:
  %empty = icmp eq i32 %member, -2
  br i1 %empty, label %drained, label %member_joined
member_joined:
  %outcome = call i32 @rt_outcome(i32 %member)
  %done = icmp eq i32 %outcome, 0
  br i1 %done, label %finished, label %halted
finished:
  %label = call ptr @rt_payload(i32 %member)
  call void @rt_emit(ptr %label)
  br label %decide
halted:
  call void @rt_emit(ptr @stopped)
  br label %decide
decide:
  %first = load i32, ptr %first_slot
  %is_first = icmp ne i32 %first, 0
  store i32 0, ptr %first_slot
  br i1 %is_first, label %winner, label %next
winner:
  call void @rt_set_cancel(i32 %members)
  br label %next
drained:
  call void @rt_complete(i32 %task, i32 0, ptr null)
  store i32 1, ptr %done_slot
  store i32 -1, ptr %state_slot
  ret void
invalid:
  unreachable
}

define void @gather_destroy(ptr %f) {
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
  %frame = call ptr @gather_init(i32 %root)
  call void @rt_task_start(i32 %root, ptr %frame)
  call void @rt_run(i32 %root)
  call void @coro_destroy(ptr %frame)
  %result = call i32 @rt_finish(i32 0)
  ret i32 %result
}
