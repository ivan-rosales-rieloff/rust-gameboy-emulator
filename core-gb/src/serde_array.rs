use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};
use std::fmt;

pub fn serialize<const N: usize, S>(arr: &[u8; N], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_bytes(arr)
}

pub fn deserialize<'de, D, const N: usize>(deserializer: D) -> Result<[u8; N], D::Error>
where
    D: Deserializer<'de>,
{
    struct ByteArrayVisitor<const N: usize>;

    impl<'de, const N: usize> Visitor<'de> for ByteArrayVisitor<N> {
        type Value = [u8; N];

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a byte array of length {}", N)
        }

        fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.try_into().map_err(|v: Vec<u8>| de::Error::invalid_length(v.len(), &self))
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.try_into().map_err(|_: std::array::TryFromSliceError| de::Error::invalid_length(v.len(), &self))
        }

        fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_bytes(v)
        }
    }

    deserializer.deserialize_byte_buf(ByteArrayVisitor::<N>)
}

pub mod u32_array {
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<const N: usize, S>(arr: &[u32; N], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(arr.as_ptr() as *const u8, N * 4)
        };
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D, const N: usize>(deserializer: D) -> Result<[u32; N], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct U32ArrayVisitor<const N: usize>;

        impl<'de, const N: usize> Visitor<'de> for U32ArrayVisitor<N> {
            type Value = [u32; N];

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "a byte array representing u32 array of length {}", N)
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.len() != N * 4 {
                    return Err(de::Error::invalid_length(v.len(), &self));
                }
                let mut arr = [0u32; N];
                unsafe {
                    std::ptr::copy_nonoverlapping(v.as_ptr(), arr.as_mut_ptr() as *mut u8, N * 4);
                }
                Ok(arr)
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.len() != N * 4 {
                    return Err(de::Error::invalid_length(v.len(), &self));
                }
                let mut arr = [0u32; N];
                unsafe {
                    std::ptr::copy_nonoverlapping(v.as_ptr(), arr.as_mut_ptr() as *mut u8, N * 4);
                }
                Ok(arr)
            }

            fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_bytes(v)
            }
        }

        deserializer.deserialize_byte_buf(U32ArrayVisitor::<N>)
    }
}

