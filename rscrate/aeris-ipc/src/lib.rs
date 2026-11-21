pub mod pingpong;

pub use pingpong::*;

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_core::{Fixed8, Symbol};

    #[test]
    fn test_struct_layout_offsets() {
        println!("MacroPayload size: {}", std::mem::size_of::<MacroPayload>());
        println!("PingPongLayout size: {}", std::mem::size_of::<PingPongLayout>());
        assert_eq!(std::mem::align_of::<MacroPayload>(), 8);
    }

    #[test]
    fn test_ping_pong_buffer_flip() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!("aeris_test_ipc_{}.dat", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));

        let mut shm = PingPongSharedMemory::create_or_open(&path).unwrap();

        let initial_payload = MacroPayload {
            updated_at_ns: 1000,
            macro_bias: 1, // Supply squeeze
            leverage_cap_fixed8: Fixed8::from_f64(2.0),
            universe_len: 2,
            universe: [
                Symbol::from_str_truncate("AVAXUSDT"),
                Symbol::from_str_truncate("SUIUSDT"),
                Symbol::default(),
                Symbol::default(),
                Symbol::default(),
                Symbol::default(),
                Symbol::default(),
                Symbol::default(),
            ],
        };

        // Pre-warm and flip
        let seq1 = shm.stage_and_flip(&initial_payload);
        assert_eq!(seq1, 1);

        let (read_seq, read_payload) = shm.read_active();
        assert_eq!(read_seq, 1);
        assert_eq!(read_payload.macro_bias, 1);
        assert_eq!(read_payload.universe[0].as_str(), "AVAXUSDT");

        // Next update pre-warmed to second buffer
        let mut second_payload = initial_payload;
        second_payload.macro_bias = 2; // Distribution
        second_payload.updated_at_ns = 2000;

        let seq2 = shm.stage_and_flip(&second_payload);
        assert_eq!(seq2, 2);

        let (read_seq2, read_payload2) = shm.read_active();
        assert_eq!(read_seq2, 2);
        assert_eq!(read_payload2.macro_bias, 2);

        let _ = std::fs::remove_file(path);
    }
}
