fn main() {
    println!("Hello, world!");
}



// TODO build a generation executable, autocxxbridge,
// which
// (1) Reads an existing .rs file to tokens
// (2) Finds include_cpp! macros and runs them through include_cpp
//     above to convert them to cxx::bridge sections
// (3) Calls cxx_gen::generate_header_and_cc(input) on the resultant
//     token stream.
// (4) Writes the output .cc and .h to files.
