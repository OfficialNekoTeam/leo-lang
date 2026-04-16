// Test: Generic function monomorphization
// Expected output:
// 42
// 99

fn identity<T>(x: T) -> T {
    return x
}

fn main() {
    println(identity<i64>(42))
    println(identity<i64>(99))
}
