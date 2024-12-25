use crate::primitives::{Address, Bytes, B256, U256};

pub(crate) fn take_address(data: &mut Vec<u8>) -> Address {
    let address: Vec<u8> = data.drain(0..20).collect();
    Address::from_slice(&address)
}

pub(crate) fn take_bytes32(data: &mut Vec<u8>) -> B256 {
    let bytes: Vec<u8> = data.drain(0..32).collect();
    B256::from_slice(bytes.as_slice())
}

pub(crate) fn take_u256(data: &mut Vec<u8>) -> U256 {
    let u256: Vec<u8> = data.drain(0..32).collect();
    U256::from_be_slice(&u256)
}

pub(crate) fn take_u64(data: &mut Vec<u8>) -> u64 {
    let u64: Vec<u8> = data.drain(0..8).collect();
    u64::from_be_bytes(u64.try_into().unwrap())
}

pub(crate) fn take_u32(data: &mut Vec<u8>) -> u32 {
    let u32: Vec<u8> = data.drain(0..4).collect();
    u32::from_be_bytes(u32.try_into().unwrap())
}

pub(crate) fn take_u16(data: &mut Vec<u8>) -> u16 {
    let u16: Vec<u8> = data.drain(0..2).collect();
    u16::from_be_bytes(u16.try_into().unwrap())
}

// pub(crate) fn take_u8(data: &mut Vec<u8>) -> u8 {
//     data.drain(0..1).next().unwrap()
// }

// pub(crate) fn take_bool(data: &mut Vec<u8>) -> bool {
//     data.drain(0..1).next().unwrap() == 1
// }

pub(crate) fn take_rest(data: &mut Vec<u8>) -> Bytes {
    let data_copy = Bytes::from(data.clone());
    data.clear();
    data_copy
}

#[cfg(test)]
mod tests {
    use crate::primitives::address;

    use super::*;

    #[test]
    fn test_take_address() {
        let expected = address!("18B06aaF27d44B756FCF16Ca20C1f183EB49111f");
        let mut data = expected.to_vec();
        let address = take_address(&mut data);
        assert_eq!(address, expected);
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_take_bytes32() {
        let mut data = vec![0u8; 32];
        let bytes = take_bytes32(&mut data);
        assert_eq!(bytes, B256::default());
    }

    #[test]
    fn test_take_u256() {
        let mut data = vec![0u8; 32];
        let u256 = take_u256(&mut data);
        assert_eq!(u256, U256::default());
    }

    #[test]
    fn test_take_u64() {
        let mut data = vec![0u8; 8];
        let u64 = take_u64(&mut data);
        assert_eq!(u64, 0);
    }

    #[test]
    fn test_take_u32() {
        let mut data = vec![0u8; 4];
        let u32 = take_u32(&mut data);
        assert_eq!(u32, 0);
    }

    #[test]
    fn test_take_u16() {
        let mut data = vec![0u8; 2];
        let u16 = take_u16(&mut data);
        assert_eq!(u16, 0);
    }

    #[test]
    fn test_take_u8() {
        let mut data = vec![0u8; 1];
        let u8 = take_u8(&mut data);
        assert_eq!(u8, 0);
    }

    #[test]
    fn test_take_bool() {
        let mut data = vec![0u8; 1];
        let boolean = take_bool(&mut data);
        assert_eq!(boolean, false);
    }
}
