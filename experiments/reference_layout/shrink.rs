use std::hint::black_box;
unsafe extern "C" {
    fn malloc_size(ptr: *const std::ffi::c_void) -> usize;
}
fn measure<const N: usize>() {
    let mut values: Vec<[u64; N]> = Vec::with_capacity(4);
    values.push([1; N]);
    values.push([2; N]);
    black_box(&mut values);
    let before = unsafe { malloc_size(values.as_ptr().cast()) };
    values.shrink_to_fit();
    black_box(&mut values);
    let after = unsafe { malloc_size(values.as_ptr().cast()) };
    let exact: Vec<_> = values.iter().copied().collect();
    black_box(&exact);
    let copied = unsafe { malloc_size(exact.as_ptr().cast()) };
    println!(
        "{},{},{},{},{},{}",
        N * 8,
        before,
        values.len(),
        values.capacity(),
        after,
        copied
    );
}
fn main() {
    println!("value_bytes,before_bytes,length,capacity,after_shrink_bytes,exact_copy_bytes");
    measure::<17>();
    measure::<16>();
    measure::<13>();
}
