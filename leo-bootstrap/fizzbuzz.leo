fn main() -> i32 {
    let i = 1;
    while i <= 100 {
        if i % 15 == 0 {
            println("FizzBuzz");
        } else if i % 3 == 0 {
            println("Fizz");
        } else if i % 5 == 0 {
            println("Buzz");
        } else {
            print(i);
            println("");
        }
        i = i + 1;
    }
    return 0;
}
