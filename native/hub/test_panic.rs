fn main() {
    // Test case: % followed by non-ASCII UTF-8
    let s = "test%😀abc";
    println!("String: {}", s);
    println!("Length: {}", s.len());
    
    // Simulate the percent_decode function
    let bytes = s.as_bytes();
    let mut i = 4; // position of '%'
    
    if bytes[i] == b'%' && i + 2 < bytes.len() {
        println!("Trying to slice [{}..{}]", i + 1, i + 3);
        println!("Slice attempt on string starting at byte {}", i + 1);
        // This would try &s[5..7] where the emoji is at bytes 5-7
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _slice = &s[i + 1..i + 3];
        }));
        match result {
            Ok(_) => println!("No panic"),
            Err(_) => println!("PANIC OCCURRED"),
        }
    }
}
