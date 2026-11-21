use aeris_core::Fixed8;
use aeris_ipc::PingPongSharedMemory;
use std::path::PathBuf;

#[test]
fn test_rust_reading_python_shared_memory() {
    let python_exe = PathBuf::from("D:/Pushp - Do not open without permission/AERIS/Python Playground/.venv/Scripts/python.exe");
    let bridge_py = PathBuf::from("D:/Pushp - Do not open without permission/AERIS/Python Playground/ipc_bridge.py");
    let shm_path = PathBuf::from("D:/Pushp - Do not open without permission/AERIS/Python Playground/data/aeris_shm.dat");

    if python_exe.exists() && bridge_py.exists() {
        let _ = std::process::Command::new(&python_exe)
            .arg(&bridge_py)
            .status();
    }

    if !shm_path.exists() {
        return;
    }

    let shm = PingPongSharedMemory::create_or_open(&shm_path).unwrap();
    let (seq, payload) = shm.read_active();

    println!("Read sequence: {}, updated_at_ns: {}, bias: {}", seq, payload.updated_at_ns, payload.macro_bias);
    println!("Leverage cap: {}", payload.leverage_cap_fixed8);
    println!("Universe count: {}", payload.universe_len);
    for i in 0..payload.universe_len as usize {
        println!("  Sym {}: {}", i, payload.universe[i].as_str());
    }

    assert!(seq >= 1);
    assert!(payload.leverage_cap_fixed8 <= Fixed8::from_f64(2.5));
}
