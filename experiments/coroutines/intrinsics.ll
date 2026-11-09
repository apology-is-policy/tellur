; Frame operations for runtime.c, which cannot name coroutine intrinsics in C.
define void @coro_resume(ptr %frame) {
  call void @llvm.coro.resume(ptr %frame)
  ret void
}
define void @coro_destroy(ptr %frame) {
  call void @llvm.coro.destroy(ptr %frame)
  ret void
}
define i32 @coro_done(ptr %frame) {
  %done = call i1 @llvm.coro.done(ptr %frame)
  %result = zext i1 %done to i32
  ret i32 %result
}
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare i1 @llvm.coro.done(ptr)
