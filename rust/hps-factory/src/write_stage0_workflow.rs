// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::erase_stage0_workflow::EraseError;
use crate::firmware::ONE_TIME_INIT_LOADER_BYTES;
use crate::hps_i2c::HpsI2c;
use crate::Config;
use crate::WriteStage0Config;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use embedded_hal::blocking::i2c::Read;
use embedded_hal::blocking::i2c::Write;
use embedded_hal::blocking::i2c::WriteRead;
use hps_interface::Hps;
use hps_interface::Stage;
use indicatif::ProgressBar;
use log::info;
use mcu_common::commands::Command;
use mcu_common::registers::Register;
use mcu_common::OptionBytesConfigRequest;
use mcu_common::Status;
use mcu_common::APPLICATION_START_ADDRESS;
use mcu_common::APPLICATION_VECTOR_TABLE_ADDRESS;
use mcu_common::FLASH_START;
use stm32_bootloader_client::MAX_READ_WRITE_SIZE;

const ROW_SIZE: usize = 256;

pub(crate) struct Stage0Writer<'a, I: Send> {
    i2c: HpsI2c<'a, I>,
    config: Config,
    write_stage0_config: WriteStage0Config,
}

enum WriteKind {
    Stage0Written,
    PermanentlyLocked,
    ExistingVersionMatched,
}

impl<'a, I, E> Stage0Writer<'a, I>
where
    I: Read<Error = E> + Write<Error = E> + WriteRead<Error = E> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    pub(crate) fn run(
        i2c: &mut I,
        config: &Config,
        write_stage0_config: &WriteStage0Config,
    ) -> Result<()> {
        let mut stage0_writer = Stage0Writer {
            i2c: HpsI2c::new(i2c, config.clone()),
            config: config.clone(),
            write_stage0_config: write_stage0_config.clone(),
        };
        if matches!(
            stage0_writer.write_stage0()?,
            WriteKind::PermanentlyLocked | WriteKind::ExistingVersionMatched
        ) {
            return Ok(());
        }
        if write_stage0_config.skip_one_time_init {
            return Ok(());
        }
        stage0_writer.write_one_time_init()?;
        stage0_writer.reset_to_stage0()?;
        stage0_writer.protect_stage0()?;
        stage0_writer.verify_protection()?;
        Ok(())
    }

    fn write_stage0(&mut self) -> Result<WriteKind> {
        let mut stage0_bytes = self.config.stage0_rom()?.to_vec();
        if self.write_stage0_config.permanent_lock && !stage0_contains_mp_key(&stage0_bytes)? {
            bail!("Stage0 does not contain the MP key");
        }
        let i2c = &mut self.i2c;
        // --check-version is an optimization to avoid repeatedly rewriting
        // stage0 on devices that are not permanently locked. It doesn't make
        // sense to skip rewriting stage0 if we're going to permanently lock the
        // device.
        if self.write_stage0_config.check_version && self.write_stage0_config.permanent_lock {
            bail!("When using --permanent-lock, --check-version is not permitted.");
        }
        let erase_required = {
            if let Ok(mut hps) = i2c.open_hps() {
                hps.perform_command(Command::Reset)?;
                hps.wait_ready()?;
                match hps.stage()? {
                    Stage::Stage0 => {
                        let version = hps.read_register(Register::HardwareVersion)?;
                        if self.write_stage0_config.check_version
                            && version == libstage0::COMBINED_VERSION
                        {
                            info!("Stage0 already has correct version: 0x{:x}", version);
                            return Ok(WriteKind::ExistingVersionMatched);
                        } else {
                            info!("Stage0 already present on device, erasing.");
                            true
                        }
                    }
                    other => bail!("HPS firmware is present, but found {:?} not stage0", other),
                }
            } else {
                // We're probably back to the system bootloader, no need to erase.
                false
            }
        };
        if erase_required {
            match crate::erase_stage0_workflow::run(i2c.i2c, self.config.clone()) {
                Ok(_) => {}
                Err(EraseError::PermanentlyLocked) => {
                    // We always consider a permanently locked stage0 to be a success.
                    info!("Stage0 already permanently locked. Nothing more to do.");
                    return Ok(WriteKind::PermanentlyLocked);
                }
                Err(EraseError::Other(other)) => return Err(other),
            }
        }
        let mut stm32 = i2c.open_bootloader()?;

        info!("Erasing MCU flash");
        stm32.erase_flash()?;

        // Pad out program out to an exact number of flash rows. We pad with 0xff
        // because that's what empty flash contains. Writing e.g. 100 bytes results
        // in the bootloader rejecting the write. Possibly it's using fast writes,
        // which are whole rows.
        stage0_bytes.resize((stage0_bytes.len() - 1) / ROW_SIZE * ROW_SIZE + 1, 0xff);

        // We write backwards rather than fowards and verify as we go. If we wrote
        // forwards, then a failure after we've written to the start of flash would
        // result in the system bootloader not starting next time we boot. This
        // could result in a bricked device that would require physical access to
        // the debug pins to unbrick.
        info!("Writing stage0");
        let bar = ProgressBar::new(stage0_bytes.len() as u64);
        let mut read_back = [0u8; MAX_READ_WRITE_SIZE];
        for (chunk_number, chunk) in stage0_bytes.chunks(MAX_READ_WRITE_SIZE).enumerate().rev() {
            let address = FLASH_START + (MAX_READ_WRITE_SIZE * chunk_number) as u32;
            stm32.write_memory(address, chunk).with_context(|| {
                format!(
                    "Failed to write {} bytes at address 0x{:x}",
                    chunk.len(),
                    address
                )
            })?;
            stm32.read_memory(address, &mut read_back[..chunk.len()])?;
            if &read_back[..chunk.len()] != chunk {
                bail!("Verification failed")
            }
            bar.inc(chunk.len() as u64);
        }
        bar.finish();
        info!("Stage0 written and verified");
        Ok(WriteKind::Stage0Written)
    }

    fn write_one_time_init(&mut self) -> Result<()> {
        self.i2c.write(
            "one-time-init",
            APPLICATION_START_ADDRESS,
            ONE_TIME_INIT_LOADER_BYTES,
        )
    }

    fn reset_to_stage0(&mut self) -> Result<()> {
        // Launch the one-time-init program. It will detect that it was launched
        // from the system bootloader and will reset into stage0.
        self.i2c
            .open_bootloader()?
            .go(APPLICATION_VECTOR_TABLE_ADDRESS)?;
        Ok(())
    }

    fn protect_stage0(&mut self) -> Result<()> {
        let i2c = &mut self.i2c;
        if i2c.open_hps()?.status()?.contains(Status::WPON) {
            bail!("Firmware write-protect needs to deassserted for this to work");
        }
        i2c.execute_non_i2c_stage1()?;
        let mut hps = i2c.open_hps()?;
        let stage = hps.stage()?;
        if stage != Stage::OneTimeInit {
            bail!(
                "Expected to be running one-time-init stage, but found: {:?}",
                stage
            );
        }
        info!("Configuring option bytes");
        let options = self.write_stage0_config.options() | OptionBytesConfigRequest::RELOAD;

        hps.write_register(Register::ConfigurationOptionBytes, options.bits())
            .context("Failed to configure option bytes")?;
        // The configure option bytes request, assuming it succeeded, will have
        // reset the device, so we need to reopen it.
        hps.wait_ready()
            .context("HPS lost after updating option bytes")?;
        hps.check_errors()?;
        let stage = hps.stage()?;
        if stage != Stage::Stage0 {
            bail!("Expected to reset to Stage0, but found {:?}", stage);
        }
        Ok(())
    }

    fn verify_protection(&mut self) -> Result<()> {
        self.i2c.execute_non_i2c_stage1()?;
        let mut hps = self.i2c.open_hps()?;
        let applied_options = OptionBytesConfigRequest::from_bits(
            hps.read_register(Register::ConfigurationOptionBytes)?,
        )
        .ok_or_else(|| anyhow!("Got invalid bits when reading applied options"))?;
        let options = self.write_stage0_config.options();
        if applied_options != options {
            bail!(
                "Options didn't apply correctly. Expected [{:?}], got [{:?}]",
                options,
                applied_options
            );
        }

        info!("Stage0 written with options: {:?}", applied_options);

        Ok(())
    }
}

impl WriteStage0Config {
    fn options(&self) -> OptionBytesConfigRequest {
        let mut options =
            OptionBytesConfigRequest::RESET_PIN | OptionBytesConfigRequest::WRITE_PROTECT;
        if self.disable_boot0 {
            options |= OptionBytesConfigRequest::DISABLE_BOOT0_PIN;
        }
        if self.permanent_lock {
            options |= OptionBytesConfigRequest::RDP2;
        } else if !self.rdp0 {
            options |= OptionBytesConfigRequest::RDP1;
        }
        options
    }
}

fn stage0_contains_mp_key(stage0_bytes: &[u8]) -> Result<bool> {
    const MP_KEY_PEM: &str = include_str!("../../mcu/stage0/keys/hps-accessory-mp.pub.pem");
    let key = ed25519_compact::PublicKey::from_pem(MP_KEY_PEM)?;
    Ok(stage0_bytes.windows(key.len()).any(|w| w == *key))
}
