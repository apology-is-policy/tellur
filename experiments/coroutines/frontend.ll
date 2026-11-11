; Explicit frontend frame dispatch. The ABI header is shared by every typed frame.
; Neither resumption nor destruction has implicit cancellation semantics.
%Frame = type { ptr, ptr, i32, i32 }
define void @coro_resume(ptr %frame) {
  %slot = getelementptr %Frame, ptr %frame, i32 0, i32 0
  %resume = load ptr, ptr %slot
  call void %resume(ptr %frame)
  ret void
}
define void @coro_destroy(ptr %frame) {
  %slot = getelementptr %Frame, ptr %frame, i32 0, i32 1
  %destroy = load ptr, ptr %slot
  call void %destroy(ptr %frame)
  ret void
}
define i32 @coro_done(ptr %frame) {
  %slot = getelementptr %Frame, ptr %frame, i32 0, i32 3
  %done = load i32, ptr %slot
  ret i32 %done
}
