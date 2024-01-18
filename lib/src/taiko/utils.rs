use rlp::{Decodable, DecoderError, Rlp};

pub fn rlp_decode_list<T>(bytes: &[u8]) -> Result<Vec<T>, DecoderError>
where
    T: Decodable,
{
    let rlp = Rlp::new(bytes);
    rlp.as_list()
}
