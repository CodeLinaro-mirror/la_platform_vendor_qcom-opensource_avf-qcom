/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use mink_interfaces::{TypedObject, IOpener, ITRebootVM};
use minkipc::MinkIPC;
use crate::{GuestAgentClient, ServiceId, ServiceId::MinkUid};
use anyhow::{anyhow, Result};
use log::{info, debug, error};
use std::time::Duration;
use std::thread;
// Hard Coded sock path
const SOCK_NAME: &str = "/dev/socket/hlos_mink_opener";

/// MinkClient represents a mink based client to the VM
/// mink_uid
pub struct MinkClient{
    pub mink_uid: u32,
    pub reboot_handle: ITRebootVM,
    pub mink_handle: MinkIPC,
}

impl GuestAgentClient for MinkClient{

    /// Connect to the userspace of the VM using Mink IPC
    /// The connect call is a blocking call. Once it has connected
    /// we can safely assume the userspace is up.
    /// Return a handle to the mink service.
    fn connect_userspace(retry:u32, timeout: u32, service_id: ServiceId) -> Result<Self>{
        let mut mink_uid = 0;
        if let MinkUid(uid) = service_id {
            mink_uid = uid;
        }
        let mut attempts = 0;
        //Connect to the service socket
        info!("Connecting to HLOS Mink Opener");
        // ToDo: Needs to catch err if the VM is not up!!
        let (mink_ipc, raw_obj) = match minkipc::MinkIPC::connect(SOCK_NAME){
                Ok((mink_ipc, raw_obj)) => {
                    info!("Successfully connected!! {:?}", raw_obj);
                    (mink_ipc, raw_obj)
                }
                Err(_e) => {
                    return Err(anyhow!("Failed to connect to Mink Hub"));
                }
            };

        while attempts < retry {
            debug!("Converting raw object to IOpener.");

            //Initialize a session with the service
            let iopener = unsafe {IOpener::from_raw(raw_obj.clone()) };
            info!("Opening TRebootVM service...");
            // `open` returns Result<Option<object::Object>>
            let reboot_raw_obj = match iopener.open(mink_uid){
                Ok(obj) => obj.unwrap(),
                Err(e) => {
                    error!("IOpener failed to open the service {:?}", e);
                    attempts += 1;
                    thread::sleep(Duration::new(
                                timeout.into(),
                                0,
                            ));
                    continue;
                }
            };
            debug!("Converting raw object to ITRebootVM.");
            let reboot_handle = unsafe { ITRebootVM::from_raw(reboot_raw_obj.clone()) };
            info!("Successfully connected to mink shutdown daemon");
            return Ok(MinkClient{
                mink_uid,
                reboot_handle,
                mink_handle: mink_ipc
            });
        }

        Err(anyhow!("Failed to connect to VM Mink Userspace"))


    }

    /// This will initiate a shutdown sequence with the VM
    /// The VM can Veto the request during which case it can provide an Err
    /// We will assume failed shutdowns means the VM refuses to shutdown.
    fn shutdown(&self) -> Result<()>{
        info!("Shutting down VM");
        let action = 0; // 0 = shutdown; 1 = restart
        let level = 0; // 0 = unforced; 1 = forced
        let res = self.reboot_handle.shutdown(action, level);
        if let Err(e) = res {
            error!("Shutdown was unsuccessful.");
            return Err(anyhow!("Shutdown Failed {:?}", e));
        }

        debug!("VM successfully shutting down");
        Ok(())
    }
}