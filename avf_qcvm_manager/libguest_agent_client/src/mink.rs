/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use mink_interfaces::{TypedObject, IOpener, ITRebootVM};
use minkipc::{CloseNotifier, CloseHandler, CloseEvent, MinkIPC};
use crate::{GuestAgentClient, IGuestNotificationCallback, ServiceId, ServiceId::MinkUid};
use anyhow::{anyhow, Result};
use log::{info, debug, error};
use std::time::Duration;
use std::thread;
use std::sync::{Arc, Mutex};
// Hard Coded sock path
const SOCK_NAME: &str = "/dev/socket/hlos_mink_opener";

/// MinkClient represents a mink based client to the VM
/// mink_uid
pub struct MinkClient{
    pub mink_uid: u32,
    pub reboot_handle: ITRebootVM,
    pub mink_handle: MinkIPC,
    pub reboot_notifier: Option<CloseNotifier<RebootServiceHandler>>,
}

pub struct RebootServiceHandler {
    pub mink_uid: u32,
    pub guest_callback: Option<Arc<Mutex<dyn IGuestNotificationCallback + Send + Sync>>>,
}
/*
impl RebootServiceHandler {
    pub fn new(vm_instance: Arc<dyn GuestAgentCallbacks + Send + Sync + >) -> Self {
        Self { vm_instance }
    }
}
*/
impl CloseHandler for RebootServiceHandler {
    fn on_close(&self, event: CloseEvent) {
        error!("Event #{:?} received", event);
        let guest_callback_cloned: Arc<Mutex<dyn IGuestNotificationCallback + Send + Sync>> = Arc::clone(self.guest_callback.as_ref().unwrap());
        let guest_callback_lock = guest_callback_cloned.lock();
        let _ = guest_callback_lock.expect("Guest callback object not found").set_reboot_handle_availability(Some(false));
    }
}

impl GuestAgentClient for MinkClient{

    /// Connect to the userspace of the VM using Mink IPC
    /// The connect call is a blocking call. Once it has connected
    /// we can safely assume the userspace is up.
    /// Return a handle to the mink service.
    fn connect_userspace(retry:u32, vm_userspace_start_timer: u32, userspace_timer: u32, service_id: ServiceId, guest_callback: Option<Arc<Mutex<dyn IGuestNotificationCallback + Send + Sync>>>) -> Result<Self>{
        let mut mink_uid = 0;
        if let MinkUid(uid) = service_id {
            mink_uid = uid;
        }
        let mut attempts = 0;

        if vm_userspace_start_timer > 0
        {
            thread::sleep(Duration::from_millis(vm_userspace_start_timer.into()));
        }
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
                    thread::sleep(Duration::from_millis(userspace_timer.into()));
                    continue;
                }
            };
            debug!("Converting raw object to ITRebootVM.");
            let reboot_handle = unsafe { ITRebootVM::from_raw(reboot_raw_obj.clone()) };
            info!("Successfully connected to mink shutdown daemon");
        
            let guest_callback_unwrap = guest_callback.unwrap();
            let guest_callback_cloned: Arc<Mutex<dyn IGuestNotificationCallback + Send + Sync>> = Arc::clone(&guest_callback_unwrap);
            let guest_callback_lock = guest_callback_cloned.lock();
            let _ = guest_callback_lock.expect("Guest callback object not found").set_reboot_handle_availability(Some(true));

            let reboot_handler = RebootServiceHandler {
                mink_uid: mink_uid,
                guest_callback: Some(guest_callback_cloned),
            };

            let reboot_notifier = match CloseNotifier::new(reboot_handler, reboot_raw_obj) {
                Ok(notifier) => {
                    info!("CloseNotifier set up successfully for TRebootVM");
                    Some(notifier)
                }
                Err(e) => {
                    error!("Failed to create CloseNotifier for ITRebootVM: {}", e);
                    None
                }
            };

            return Ok(MinkClient{
                mink_uid,
                reboot_handle,
                mink_handle: mink_ipc,
                reboot_notifier,
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
