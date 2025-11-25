/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
pub mod mink;
pub mod vsock;
use anyhow::{Result};

/// This enum encapsulates different types of IPC.
/// For Mink there is a MinkUid and for Vsock there is a VsockPort
/// This also allows for easy expansion, ex.
///     Vsock(u32, u32) -> CID and Port
///     BinderRPC(String, u32) -> Service name and port
pub enum ServiceId{
    MinkUid(u32),
    VsockPort(u32),
}

/// A common trait that each type of IPC guest agent needs to implement
/// There needs to be a way to signify when the userspace is ready. Also there
/// has to be a method to shutdown the VM.
pub trait GuestAgentClient{
    /// Guest Agents should notify when userspace is up and ready
    /// There is a timeout and retries for connecting to userspace
    fn connect_userspace(retry:u32, vm_userspace_start_timer: u32, timeout: u32, service_id: ServiceId) -> Result<Self>
         where Self: std::marker::Sized;

    /// Every Guest Agent needs to be able to shutdown their VM
    fn shutdown(&self) -> Result<()>;
}
