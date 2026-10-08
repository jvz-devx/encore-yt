//! Fixed-size shader uniforms, encoded without frame-time heap storage.

pub(crate) struct Uniform<const N: usize> {
    bytes: [u8; N],
    written: usize,
}

impl<const N: usize> Uniform<N> {
    pub fn new() -> Self {
        Self {
            bytes: [0; N],
            written: 0,
        }
    }

    pub fn push(&mut self, value: f32) {
        self.bytes[self.written..self.written + 4].copy_from_slice(&value.to_ne_bytes());
        self.written += 4;
    }

    pub fn extend(&mut self, values: impl IntoIterator<Item = f32>) {
        for value in values {
            self.push(value);
        }
    }

    pub fn finish(self) -> [u8; N] {
        debug_assert_eq!(self.written, N, "shader uniform layout");
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::Uniform;

    #[test]
    fn fields_keep_their_order_and_native_float_encoding() {
        let mut uniform = Uniform::<16>::new();
        uniform.extend([1.25, -2.0, 0.0]);
        uniform.push(0.75);
        let expected: Vec<u8> = [1.25_f32, -2.0, 0.0, 0.75]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        assert_eq!(uniform.finish().as_slice(), expected);
    }
}
