// Licensed under the Apache-2.0 license

use super::hardware::{HaceController, HashAlgo, HACE_SG_EN};
use proposed_traits::mac::{Error, ErrorKind, ErrorType, MacAlgorithm, MacInit, MacOp};

pub trait IntoHashAlgo {
    fn to_hash_algo() -> HashAlgo;
}

// Digest types for different output sizes
pub struct Digest48(pub [u8; 48]);

impl Default for Digest48 {
    fn default() -> Self {
        Digest48([0u8; 48])
    }
}

impl AsRef<[u8]> for Digest48 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for Digest48 {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

pub struct Digest64(pub [u8; 64]);

impl Default for Digest64 {
    fn default() -> Self {
        Digest64([0u8; 64])
    }
}

impl AsRef<[u8]> for Digest64 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for Digest64 {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

// Algorithm implementations
#[derive(Debug, Default)]
pub struct HwSha256;

impl MacAlgorithm for HwSha256 {
    const OUTPUT_BITS: usize = 256;
    type MacOutput = [u8; 32];
    type Key = [u8; 32];
}

impl IntoHashAlgo for HwSha256 {
    fn to_hash_algo() -> HashAlgo {
        HashAlgo::SHA256
    }
}

#[derive(Debug, Default)]
pub struct HwSha384;

impl MacAlgorithm for HwSha384 {
    const OUTPUT_BITS: usize = 384;
    type MacOutput = Digest48;
    type Key = Digest48;
}

impl IntoHashAlgo for HwSha384 {
    fn to_hash_algo() -> HashAlgo {
        HashAlgo::SHA384
    }
}

#[derive(Debug, Default)]
pub struct HwSha512;

impl MacAlgorithm for HwSha512 {
    const OUTPUT_BITS: usize = 512;
    type MacOutput = Digest64;
    type Key = Digest64;
}

impl IntoHashAlgo for HwSha512 {
    fn to_hash_algo() -> HashAlgo {
        HashAlgo::SHA512
    }
}

// Context implementation
pub struct OpContextImpl<'a, A> {
    controller: &'a mut HaceController,
    _algorithm: core::marker::PhantomData<A>,
}

// Error type
#[derive(Debug)]
pub struct MacError(pub ErrorKind);

impl Error for MacError {
    fn kind(&self) -> ErrorKind {
        self.0
    }
}

impl From<ErrorKind> for MacError {
    fn from(kind: ErrorKind) -> Self {
        MacError(kind)
    }
}

// ErrorType implementation for HaceController
impl ErrorType for HaceController {
    type Error = MacError;
}

// MacInit implementation for HaceController
impl<A> MacInit<A> for HaceController
where
    A: MacAlgorithm + IntoHashAlgo,
    A::Key: AsRef<[u8]>,
    A::MacOutput: Default + AsMut<[u8]>,
{
    type OpContext<'a> = OpContextImpl<'a, A>
    where
        Self: 'a;

    fn init<'a>(&'a mut self, _algorithm: A, key: &A::Key) -> Result<Self::OpContext<'a>, Self::Error>
    {
        self.setup_context::<A>(key)?;
        Ok(OpContextImpl {
            controller: self,
            _algorithm: core::marker::PhantomData,
        })
    }
}

// Helper methods for HaceController
impl HaceController {
    fn setup_context<A>(&mut self, key: &A::Key) -> Result<(), MacError>
    where
        A: MacAlgorithm + IntoHashAlgo,
        A::Key: AsRef<[u8]>,
    {
        let algo = A::to_hash_algo();
        self.algo = algo;

        let block_size = algo.block_size();
        let key_bytes = key.as_ref();

        // Ensure the key length is valid
        if key_bytes.len() > block_size {
            return Err(MacError(ErrorKind::InvalidInputLength));
        }

        self.init_context().map_err(|_| MacError(ErrorKind::UpdateError))?;

        // Key preprocessing
        {
            let ctx = self.ctx_mut();
            
            // Initialize pads with zeros
            ctx.ipad.fill(0);
            ctx.opad.fill(0);

            // Copy key into both pads and apply XOR operations
            for (i, &key_byte) in key_bytes.iter().enumerate() {
                ctx.ipad[i] = key_byte ^ 0x36;
                ctx.opad[i] = key_byte ^ 0x5c;
            }

            // Apply XOR to remaining bytes up to block size
            for i in key_bytes.len()..block_size {
                ctx.ipad[i] ^= 0x36;
                ctx.opad[i] ^= 0x5c;
            }
        }

        Ok(())
    }
}

// ErrorType implementation for OpContextImpl
impl<A> ErrorType for OpContextImpl<'_, A>
where
    A: MacAlgorithm + IntoHashAlgo,
{
    type Error = MacError;
}

// MacOp implementation for OpContextImpl
impl<A> MacOp for OpContextImpl<'_, A>
where
    A: MacAlgorithm + IntoHashAlgo,
    A::MacOutput: Default + AsMut<[u8]>,
{
    type Output = A::MacOutput;

    fn update(&mut self, input: &[u8]) -> Result<(), Self::Error> {
        let ctrl: &mut HaceController = self.controller;
        let algo = ctrl.algo;
        let block_size = algo.block_size();
        let digest_size = algo.digest_size();
        let mut bufcnt: u32;

        {
            let ctx = ctrl.ctx_mut();
            ctx.digcnt[0] = block_size as u64;
            ctx.bufcnt =
                u32::try_from(block_size).map_err(|_| MacError(ErrorKind::InvalidInputLength))?;

            // H(ipad + input)
            let ipad = &ctx.ipad[..block_size];
            ctx.buffer[..algo.block_size()].copy_from_slice(ipad);
            ctx.buffer[algo.block_size()..(algo.block_size() + input.len())].copy_from_slice(input);
            ctx.digcnt[0] += input.len() as u64;
            ctx.bufcnt +=
                u32::try_from(input.len()).map_err(|_| MacError(ErrorKind::InvalidInputLength))?;
            ctx.method &= !HACE_SG_EN; // Disable SG mode for key hashing
        }

        ctrl.fill_padding(0);
        bufcnt = ctrl.ctx_mut().bufcnt;
        ctrl.copy_iv_to_digest();
        ctrl.start_hash_operation(bufcnt);
        let slice =
            unsafe { core::slice::from_raw_parts(ctrl.ctx_mut().digest.as_ptr(), digest_size) };

        // H(opad + H(opad + hash sum))
        {
            let ctx = ctrl.ctx_mut();
            ctx.digcnt[0] = block_size as u64 + digest_size as u64;
            ctx.bufcnt = u32::try_from(block_size + digest_size)
                .map_err(|_| MacError(ErrorKind::UpdateError))?;
            ctx.buffer[..block_size].copy_from_slice(&ctx.opad[..block_size]);
            ctx.buffer[block_size..(block_size + digest_size)].copy_from_slice(slice);
        }
        ctrl.fill_padding(0);
        bufcnt = ctrl.ctx_mut().bufcnt;
        ctrl.copy_iv_to_digest();
        ctrl.start_hash_operation(bufcnt);

        Ok(())
    }

    fn finalize(self) -> Result<Self::Output, Self::Error> {
        let digest_size = self.controller.algo.digest_size();
        let ctx = self.controller.ctx_mut();

        let slice = unsafe { core::slice::from_raw_parts(ctx.digest.as_ptr(), digest_size) };

        let mut output = A::MacOutput::default();
        output.as_mut()[..digest_size].copy_from_slice(slice);

        // Note: cleanup_context is not available in hardware module, 
        // but the context will be cleaned up when the controller is dropped

        Ok(output)
    }
}
