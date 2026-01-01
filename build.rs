fn main() {
    let lua_lib = std::env::var("LUA_LIB").expect("please set LUA_LIB");
    let lua_lib_name = match std::env::var("LUA_LIB_NAME") {
        Ok(o) => o,
        Err(_) => "lua".to_string(),
    };
    println!("cargo:rustc-link-lib={lua_lib_name}");
    println!("cargo:rustc-link-search={lua_lib}");
}
