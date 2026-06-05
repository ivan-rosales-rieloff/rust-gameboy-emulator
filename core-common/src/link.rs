use std::io;

/// A hardware link endpoint for exchanging a single serial byte.
///
/// The Game Boy serial link uses a byte-at-a-time exchange between peers.
/// Implementors provide the transport-specific code to send a byte and
/// receive the reply from the remote device.
pub trait LinkEndpoint: Send + std::fmt::Debug {
    /// Transfer one byte over the link cable.
    ///
    /// The returned byte is the value received from the remote peer.
    fn transfer_byte(&mut self, byte: u8) -> io::Result<u8>;
}
