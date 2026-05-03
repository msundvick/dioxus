fn main() {
    let build = cxx_build::bridge("src/lib.rs").std("c++14").clone();
    cxx_build::compile_as_shared_lib(build, "cdylib_cxx");
}
