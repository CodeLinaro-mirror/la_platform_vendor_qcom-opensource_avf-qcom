/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

use binder::{
    BinderFeatures, DeathRecipient, IBinder, Interface, Strong,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
    Mutex
};
use std::{thread, time};
use vendor_qti_AvfQcvmManager::aidl::vendor::qti::AvfQcvmManager::{
    IAvfQcvmManager::{
        IAvfQcvmManager, BpAvfQcvmManager
    },
    IVirtualMachine::IVirtualMachine,
    IVirtualMachineCallback::{
        BnVirtualMachineCallback, IVirtualMachineCallback,
    },
    VirtualMachineError::VirtualMachineError,
};
use vendor_qti_AvfQcvmManager::binder;

struct VirtualMachineCallback {
    name: String,
    vm_state: Arc<Mutex<VmState>>,
}

impl Interface for VirtualMachineCallback {}

impl IVirtualMachineCallback for VirtualMachineCallback {
    fn onStarting(&self) -> Result<(), binder::Status> {

        println!("Callback Received: VM '{}' is starting", self.name);
         *self.vm_state.lock().unwrap() = VmState::Started;
        Ok(())
    }

    fn onStopped(&self) -> Result<(), binder::Status> {
        *self.vm_state.lock().unwrap() = VmState::Stopped;
        println!("Callback Received: VM '{}' has stopped", self.name);
        Ok(())
    }

    fn onUserspaceReady(&self) -> Result<(), binder::Status> {
        *self.vm_state.lock().unwrap() = VmState::UserspaceReady;
        println!("Callback Received: VM '{}' userspace is ready", self.name);
        Ok(())
    }

    fn onShutdownInitiated(&self) -> Result<(), binder::Status> {
        *self.vm_state.lock().unwrap() = VmState::ShutdownInitiated;
        println!("Callback Received: VM '{}' shutdown initiated", self.name);
        Ok(())
    }

    fn onCrashed(&self) -> Result<(), binder::Status> {
        *self.vm_state.lock().unwrap() = VmState::Crashed;
        println!("Callback Received: VM '{}' has crashed", self.name);
        Ok(())
    }

    fn onError(&self, error: VirtualMachineError) -> Result<(), binder::Status> {
        println!("Callback Received: VM '{}' encountered an error: {:?}", self.name, error);
        Ok(())
    }
}

#[derive(Debug)]
enum VmState {
    NotStarted,
    Started,
    Stopped,
    Crashed,
    ShutdownInitiated,
    UserspaceReady,
}

struct VirtualMachine {
    name: String,
    vm: Strong<dyn IVirtualMachine>,
    vm_callback: Strong<dyn IVirtualMachineCallback>,
    vm_state: Arc<Mutex<VmState>>,
}

struct StateMachine {
    service: Strong<dyn IAvfQcvmManager>,
    vms: Vec<VirtualMachine>,
    _death_recipient: DeathRecipient,
    status: Arc<AtomicBool>,
}

impl StateMachine {
    fn new(service: Strong<dyn IAvfQcvmManager>) -> Self {
        let status = Arc::new(AtomicBool::new(true));
        let status_clone = status.clone();
        let mut death_recipient = DeathRecipient::new(move || {
            println!("VirtualizationService Died!");
            status_clone.fetch_and(false, Ordering::SeqCst);
        });
        let mut svc = service.as_binder();
        svc.link_to_death(&mut death_recipient).unwrap();
        Self {
            service,
            vms: Vec::new(),
            _death_recipient: death_recipient,
            status,
        }
    }

    fn list_available_vms(&self) {
        match self.service.availableVms() {
            Ok(vm_list) => {
                if vm_list.is_empty() {
                    println!("No available VMs found.");
                    return;
                }
                println!("\nAvailable VMs:");
                for (index, vm) in vm_list.iter().enumerate() {
                    println!("{}. {}", index + 1, vm.name);
                }
            }
            Err(e) => println!("Failed to retrieve available VMs: {}", e),
        }
    }

    fn get_vm(&mut self) {
        self.list_available_vms();
        let mut vm_choice = String::new();
        println!("Select a VM by number: ");
        if std::io::stdin().read_line(&mut vm_choice).is_ok() {
            match vm_choice.trim().parse::<usize>() {
                Ok(index) => {
                    if let Ok(vm_list) = self.service.availableVms() {
                        if index > 0 && index <= vm_list.len() {
                            let name = vm_list[index - 1].name.clone();
                            let vm = self.service.getVm(&name).ok();
                            if let Some(vm) = vm {
                                let vm_state = Arc::new(Mutex::new(VmState::NotStarted));
                                let vm_callback = BnVirtualMachineCallback::new_binder(
                                VirtualMachineCallback {
                                        name: name.clone(),
                                        vm_state: vm_state.clone(),
                                },
                                BinderFeatures::default(),
                            );
                                self.vms.push(VirtualMachine {
                                    name: name.clone(),
                                    vm,
                                    vm_callback,
                                    vm_state,
                                });
                                println!("Selected VM '{}'", name);
                            } else {
                                println!("Failed to get VM '{}'", name);
                    }
                        } else {
                            println!("Invalid selection.");
            }
        }
    }
                _ => println!("Invalid input."),
            }
        }
    }

    fn operate_vm(&mut self) {
        let mut option = String::new();
        println!("\n------Options------");
        println!("1. Get VM Info");
        println!("2. Start VM");
        println!("3. Force Stop VM");
        println!("4. Request VM Stop");
        println!("5. Get VM State");
        println!("6. Drop VM");
        println!("7. exit");
        println!("-------------------");
        if std::io::stdin().read_line(&mut option).is_ok() {
            match option.trim().parse::<i32>() {
                Ok(1) => {
                    if let Some(vm) = self.vms.last() {
                        match vm.vm.getVmInfo() {
                            Ok(info) => println!(
                                "VM Info - Name: {}, Early VM Enabled: {}, Enabled: {}, Force Stop Supported: {}, vCPUs: {}, CMA Size: {} MB, SWIOTLB Size: {} MB, Total Memory: {} MB, VM ID: {}, Mink UID: {}",
                                info.name,
                                info.early_vm,
                                info.enabled,
                                info.force_stop,
                                info.num_vcpus,
                                info.cma_size,
                                info.swiotlb_size,
                                info.total_memory,
                                info.vm_id,
                                info.mink_uid
                            ),
                            Err(e) => println!("Failed to retrieve VM Info: {}", e),
                        }
                    } else {
                        println!("No VM selected.");
                    }
                }
                Ok(2) => {
                    if let Some(vm) = self.vms.last() {
                        if matches!(*vm.vm_state.lock().unwrap(), VmState::Started)
                            || matches!(*vm.vm_state.lock().unwrap(), VmState::UserspaceReady) {
                                println!("VM already started.");
                        } else if vm.vm.start(&vm.vm_callback).is_ok() {
                            println!("Triggered VM: '{}' start", vm.name);
                            } else {
                                println!("Failed to trigger VM start.");
                            }
                    }
                }
                Ok(3) => {
                    if let Some(vm) = self.vms.last() {
                        if !matches!(*vm.vm_state.lock().unwrap(), VmState::Started)
                            && !matches!(*vm.vm_state.lock().unwrap(), VmState::UserspaceReady) {
                            println!("Please register for callback first by calling start API.");
                        } else if matches!(*vm.vm_state.lock().unwrap(), VmState::Stopped){
                            println!("VM '{}' already stopped", vm.name);
                        } else if vm.vm.stop(&vm.vm_callback).is_ok() {
                            println!("Triggered VM: '{}' stop", vm.name);
                            *vm.vm_state.lock().unwrap() = VmState::Stopped;
                        } else {
                            println!("Failed to trigger VM stop.");
                        }
                    }
                }
                Ok(4) => {
                    if let Some(vm) = self.vms.last() {
                        if !matches!(*vm.vm_state.lock().unwrap(), VmState::Started)
                            && !matches!(*vm.vm_state.lock().unwrap(), VmState::UserspaceReady) {
                            println!("Please register for callback first by calling start API.");
                        } else if matches!(*vm.vm_state.lock().unwrap(), VmState::Stopped){
                            println!("VM '{}' already stopped", vm.name);
                        } else if vm.vm.request_stop(&vm.vm_callback).is_ok() {
                            println!("Requested VM: '{}' stop", vm.name);
                            *vm.vm_state.lock().unwrap() = VmState::Stopped;
                        } else {
                            println!("Failed to request VM stop.");
                        }
                    }
                }
                Ok(5) => {
                    if let Some(vm) = self.vms.last() {
                        if matches!(*vm.vm_state.lock().unwrap(), VmState::NotStarted) {
                        println!("Please register for callback first by calling start API.");
                    } else {
                            let state = vm.vm_state.lock().unwrap();
                        println!("Current VM State: {:?}", *state);
                    }
                }
                }
                Ok(6) => {
                    println!("Dropping VM.");
                    self.vms.pop();
                }
                Ok(7) => {
                    println!("Exiting");
                    std::process::exit(0);
                }
                _ => println!("Invalid option."),
            };
        }
    }

    fn run(&mut self) -> Result<(), ()> {
        loop {
            if !self.status.load(Ordering::SeqCst) {
                return Err(());
            } else if !self.vms.is_empty() {
                self.operate_vm();
            } else {
                self.get_vm();
            };
            thread::sleep(time::Duration::from_millis(500));
        }
    }
}

fn main() {
    binder::ProcessState::start_thread_pool();

    let virt_service: Strong<dyn IAvfQcvmManager> =
        binder::get_interface(&format!("{}/default", BpAvfQcvmManager::get_descriptor()))
            .expect("Failed to get service.");

    let mut machine = StateMachine::new(virt_service);
    if let Err(_) = machine.run() {
        println!("Service Dropped. Retry? (Y/N)");
        let mut act = String::new();
        if std::io::stdin().read_line(&mut act).is_ok() {
            match act.trim().to_uppercase().as_str() {
                "Y" | "YES" => main(),
                "N" | "NO" => {}
                _ => {}
            };
        }
    }
}