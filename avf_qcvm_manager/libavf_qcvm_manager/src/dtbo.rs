/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use nix::unistd::{chown, Uid};
use anyhow::{anyhow, ensure, Context, Result};
use log::{info, warn, debug};
use std::fs::{File, set_permissions, create_dir, remove_dir_all, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::raw::uid_t;
use std::io::{ Write, Read, Seek, SeekFrom};
use zerocopy::{
    byteorder::{BigEndian, U32},
    FromBytes,
};

use std::path::{PathBuf};

pub const DT_TABLE_MAGIC: u32 = 0xd7b7ab1e;
/// Directory in which to write disk image files used while running VMs.
pub const TEMPORARY_DIRECTORY: &str = "/data/vendor/qtvm_dtbo";


pub fn get_or_create_common_dir() -> Result<PathBuf> {
    let path = PathBuf::from(TEMPORARY_DIRECTORY);
    info!("path: {:?}", path);
    if !path.exists() {
        create_temporary_directory(&path, None)?;
    }
    Ok(path)
}

pub fn create_temporary_directory(path: &PathBuf, requester_uid: Option<uid_t>) -> Result<()> {
    // Directory may exist if previous attempt to create it had failed.
    // Delete it before trying again.
    if path.as_path().exists() {
        remove_temporary_dir(path).unwrap_or_else(|e| {
            warn!("Could not delete temporary directory {:?}: {}", path, e);
        });
    }
    // Create directory.
    create_dir(path).with_context(|| format!("Could not create temporary directory {:?}", path))?;
    // If provided, change ownership to client's UID but system's GID, and permissions 0700.
    // If the chown() fails, this will leave behind an empty directory that will get removed
    // at the next attempt, or if virtualizationservice is restarted.
    if let Some(uid) = requester_uid {
        chown(path, Some(Uid::from_raw(uid)), None).with_context(|| {
            format!("Could not set ownership of temporary directory {:?}", path)
        })?;
    }
    Ok(())
}

/// Removes a directory owned by a different user by first changing its owner back
/// to VirtualizationService.
pub fn remove_temporary_dir(path: &PathBuf) -> Result<()> {
    ensure!(path.as_path().is_dir(), "Path {:?} is not a directory", path);
    chown(path, Some(Uid::current()), None)?;
    set_permissions(path, Permissions::from_mode(0o700))?;
    remove_dir_all(path)?;
    Ok(())
}

#[repr(C)]
#[derive(Debug, FromBytes)]
pub struct DtTableHeader {
    /// DT_TABLE_MAGIC
    magic: U32<BigEndian>,
    /// includes dt_table_header + all dt_table_entry and all dtb/dtbo
    _total_size: U32<BigEndian>,
    /// sizeof(dt_table_header)
    header_size: U32<BigEndian>,
    /// sizeof(dt_table_entry)
    dt_entry_size: U32<BigEndian>,
    /// number of dt_table_entry
    dt_entry_count: U32<BigEndian>,
    /// offset to the first dt_table_entry from head of dt_table_header
    dt_entries_offset: U32<BigEndian>,
    /// flash page size we assume
    _page_size: U32<BigEndian>,
    /// DTBO image version, the current version is 0. The version will be
    /// incremented when the dt_table_header struct is updated.
    _version: U32<BigEndian>,
}

#[repr(C)]
#[derive(Debug, FromBytes)]
pub struct DtTableEntry {
    /// size of each DT
    dt_size: U32<BigEndian>,
    /// offset from head of dt_table_header
    dt_offset: U32<BigEndian>,
    /// optional, must be zero if unused
    _id: U32<BigEndian>,
    /// optional, must be zero if unused
    _rev: U32<BigEndian>,
    /// optional, must be zero if unused
    _custom: [U32<BigEndian>; 4],
}

pub fn get_dt_table_header(file: &mut File) -> Result<DtTableHeader> {
    let values = read_values(file, size_of::<DtTableHeader>(), 0)?;
    let dt_table_header = DtTableHeader::read_from(values.as_slice())
        .context("DtTableHeader is invalid")?;
    if dt_table_header.magic.get() != DT_TABLE_MAGIC
        || dt_table_header.header_size.get() as usize != size_of::<DtTableHeader>()
    {
        return Err(anyhow!("DtTableHeader is invalid"));
    }
    Ok(dt_table_header)
}

pub fn get_dt_table_entry(
    file: &mut File,
    header: &DtTableHeader,
    index: u32,
) -> Result<DtTableEntry> {
    if index >= header.dt_entry_count.get() {
        return Err(anyhow!("Invalid dtbo index {index}"));
    }
    let Some(prev_dt_entry_total_size) = header.dt_entry_size.get().checked_mul(index) else {
        return Err(anyhow!("Unexpected arithmetic result"));
    };
    let Some(dt_entry_offset) =
        prev_dt_entry_total_size.checked_add(header.dt_entries_offset.get())
    else {
        return Err(anyhow!("Unexpected arithmetic result"));
    };
    let values = read_values(file, size_of::<DtTableEntry>(), dt_entry_offset.into())?;
    let dt_table_entry = DtTableEntry::read_from(values.as_slice())
        .with_context(|| format!("DtTableEntry at index {index} is invalid."))?;
    Ok(dt_table_entry)
}

pub fn read_values(file: &mut File, size: usize, offset: u64) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))
        .context("Cannot seek the offset")?;
    let mut buffer = vec![0_u8; size];

    file.read_exact(&mut buffer)
        .context("Failed to read buffer")?;

    Ok(buffer)
}

pub fn copy_vm_full_dtbo_from_img(
    dtbo_img_file: &mut File,
    entry: &DtTableEntry,
    path: PathBuf,
    dtbo_file: &mut File,
) -> Result<()> {
    let dt_size = entry
        .dt_size
        .get()
        .try_into()
        .context("Failed to convert type")?;
    let buffer = read_values(dtbo_img_file, dt_size, entry.dt_offset.get().into())?;
    debug!("dtbo buffer: {:?}", buffer);

    dtbo_file
        .write_all(&buffer)
        .context("Failed to write dtbo file")?;

    let mut data = vec![0_u8; dt_size.try_into().unwrap()];
    let mut tmp = File::open(path).expect("Failed to open file in read mode");
    tmp.read_exact(&mut data).expect("Failed to read data");
    debug!("CHECK DTBO: {:?}", String::from_utf8_lossy(&data));
    Ok(())
}
