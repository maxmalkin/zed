// Calls are disabled in this build; audio playback/recording needs no WebRTC echo processor.
mod fake_implementation {
    #[derive(Clone, Default)]
    pub struct EchoCanceller;

    impl EchoCanceller {
        pub fn process_reverse_stream(&mut self, _buf: &mut [i16]) {}
        pub fn process_stream(&mut self, _buf: &mut [i16]) -> anyhow::Result<()> {
            Ok(())
        }
    }
}

pub use fake_implementation::EchoCanceller;
