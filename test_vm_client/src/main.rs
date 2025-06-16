/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

#![allow(unused_imports)]
#![allow(clippy::empty_loop)]
use binder::{
    BinderFeatures, DeathRecipient, IBinder, Interface, Strong, Result
};
use log::LevelFilter;

use vendor_qti_AvfQcvmManager::aidl::vendor::qti::AvfQcvmManager::{
    IAvfQcvmManager::{
        BnAvfQcvmManager, IAvfQcvmManager, BpAvfQcvmManager
    }, VmInfo::VmInfo, IVirtualMachine::IVirtualMachine,
    IVirtualMachineCallback::{
        IVirtualMachineCallback, BnVirtualMachineCallback},
    VirtualMachineError::VirtualMachineError,
};

struct VirtualMachineCallback {
    name: String,
}

impl VirtualMachineCallback{
    pub fn to_binder(self) -> Strong<dyn IVirtualMachineCallback> {
        BnVirtualMachineCallback::new_binder(self, BinderFeatures::default())
    }
}


use std::thread;
use std::time::Duration;


impl Interface for VirtualMachineCallback {}

impl IVirtualMachineCallback for VirtualMachineCallback {
    fn onStarting(&self) -> Result<()> {
        println!("\n-------------------------------");
        println!("CB Recvd for VM: {}, New State: starting", self.name);
        println!("-------------------------------\n");
        Ok(())
    }

    fn onUserspaceReady(&self) -> Result<()> {
        println!("\n-------------------------------");
        println!("CB Recvd for VM: {}, New State: Userspace Ready", self.name);
        println!("-------------------------------\n");
        Ok(())
    }

    fn onShutdownInitiated(&self) -> Result<()> {
        println!("\n-------------------------------");
        println!("CB Recvd for VM: {}, New State: shutdown initiated", self.name);
        println!("-------------------------------\n");
        Ok(())
    }

    fn onStopped(&self) -> Result<()> {
        println!("\n-------------------------------");
        println!("CB Recvd for VM: {}, New State: stopped", self.name);
        println!("-------------------------------\n");
        Ok(())
    }

    fn onCrashed(&self) -> Result<()> {
        println!("\n-------------------------------");
        println!("CB Recvd for VM: {}, New State: Crashed", self.name);
        println!("-------------------------------\n");
        Ok(())
    }

    fn onError(&self, error: VirtualMachineError) -> Result<()>{
        println!("\n-------------------------------");
        println!("CB Recvd for VM: {}, New State: Error {:?}", self.name, error);
        println!("-------------------------------\n");

        Ok(())
    }
}

fn main() {
    binder::ProcessState::start_thread_pool();

    let _init_success = logger::init(
        logger::Config::default()
            .with_tag_on_device("oemvm test client")
            .with_max_level(LevelFilter::Debug),
    );


    let virt_service: Strong<dyn IAvfQcvmManager> =
        binder::get_interface(&format!(
            "{}/default",
            BpAvfQcvmManager::get_descriptor()
        ))
        .expect("Failed to get service.");

    let vm = virt_service.getVm("oemvm").expect("Failed to get VM");
    let callback = VirtualMachineCallback{name: String::from("oemvm")};
    let cb_binder = callback.to_binder();
    vm.start(&cb_binder).expect("Failed to start");

    let five_seconds = Duration::from_secs(20);
    println!("Sleeping for 5 seconds...");
    thread::sleep(five_seconds);
    println!("Awake now!");
    vm.request_stop(&cb_binder).expect("Failed to stop");

    loop{}


}