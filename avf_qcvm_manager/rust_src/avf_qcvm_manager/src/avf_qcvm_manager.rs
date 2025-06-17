/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
#![allow(non_snake_case)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_mut)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_variables)]
use binder::{
    BinderFeatures, ExceptionCode, Interface, Status, Strong, ThreadState, ParcelFileDescriptor, Result as BinderResult
};
use std::{collections::HashMap, sync::{Arc, Mutex, Weak}, fmt::Debug};
use log::{warn, info, error};
use serde_json::Value;
use std::fs::{File, set_permissions, create_dir, remove_dir_all, remove_file, Permissions};
use vendor_qti_AvfQcvmManager::aidl::vendor::qti::AvfQcvmManager::{
    IAvfQcvmManager::{
        BnAvfQcvmManager, IAvfQcvmManager, BpAvfQcvmManager
    }, VmInfo::VmInfo, IVirtualMachine::IVirtualMachine,
};
use rustutils::system_properties;
use anyhow::{anyhow, bail, ensure, Context, Result};

use avf_llndk_bindgen::{AVirtualizationService, AVirtualizationService_create};

use crate::virtual_machine::{create_vm_info, VirtualMachine,
    VmConfig, State, VmInstance};

const VM_CONFIG_PATH: &str = "/vendor/etc/qcvm_config.json";

/// Core Implementation of IAvfQcvmManager
/// The vm_map needs to be an Arc pointer since its shared between threads
///     Since the handle to the VM is not changing, we don't need a mutex. The VM instance
///     will manage the multi threading
/// The VM info is just a general list of VM information for Client's benefit
///     Ideally clients, should never read the config.json at runtime
pub struct AvfQcvmManager {
    pub vm_map: Arc<HashMap<VmConfig, Option<Strong<dyn IVirtualMachine>> >>,
    pub vm_infos: Vec<VmInfo>,
}

impl Interface for AvfQcvmManager{

}

impl AvfQcvmManager{
    pub fn to_binder(self) -> Strong<dyn IAvfQcvmManager> {
        BnAvfQcvmManager::new_binder(self, BinderFeatures::default())
    }

    pub fn get_descriptor() -> String {
        BpAvfQcvmManager::get_descriptor().to_string()
    }

    /// This creates a new instance of the AvfQcvmManager
    /// It will populate the VmConfigs and hashmap accordingly
    /// It will also set up the Virtual machine instances
    pub fn new() -> Result<Self>{
        let vm_configs = parse_vm_config_json()?;
        // This list will be the internal copy of the vm_infos
        let mut vm_infos = Vec::<VmInfo>::new();
        for vm_config in &vm_configs {
            let vm_info = create_vm_info(vm_config)?;
            vm_infos.push(vm_info);
        }

        let mut vm_map = HashMap::<VmConfig, Option<Strong<dyn IVirtualMachine>> >::new();
        // let mut virtmgr_handle = Self::get_virtmgr()?;
        for vm_config in vm_configs{
            let state = Arc::new(Mutex::new(State::STOPPED));
            let mut vm_instance = VmInstance{
                vm_config: vm_config.clone(),
                state: Arc::downgrade(&state),
                ..Default::default()
            };
            // let _ = vm_instance.create_avf_config();
            // vm_instance.avf_handle.virtmgr_service = Some(virtmgr_handle);
            let vm = VirtualMachine{
                vm_instance: Arc::new(Mutex::new(vm_instance)),
                main_state: state,
            };
            vm_map.insert(vm_config, Some(vm.to_binder()));

        }

        Ok(AvfQcvmManager { vm_map: Arc::new(vm_map), vm_infos })
    }



}

impl IAvfQcvmManager for AvfQcvmManager{

    /// Clients can query the VMs that are available to boot via this HAL
    fn availableVms(&self) -> BinderResult<Vec<VmInfo>>{
        /* This list will be sent to clients and released from this process
        ownership. Need to clone  */
        let available_vm_infos = to_binder_result(clone_vm_infos(&self.vm_infos))?;
        Ok(available_vm_infos)
    }

    /// Get a binder instance of a particular VM. Use the VM name
    /// If unsure, query the avialable VMs first.
    fn getVm(&self, vm_name: &str) -> BinderResult<binder::Strong<dyn IVirtualMachine>>{
        // let mut vm = VirtualMachine::new();
        // Get an iterator over the keys
        let map = Arc::clone(&self.vm_map);
        let keys: Vec<VmConfig> = map.keys().cloned().collect();

        for key in keys{
            // Find the VM in the map
            if key.name == String::from(vm_name){
                if let Some(vm) = &map.get(&key){
                    // As_ref makes a clonable reference to the binder object
                    if let Some(vm_binder) = vm.as_ref(){
                        // Make sure every client is using a reference to the same binder object
                        return Ok(vm_binder.clone());
                    }

                }
            }
        }
        error!("getVm: {vm_name} was not found");
        return Err(Status::new_exception_str(
            ExceptionCode::UNSUPPORTED_OPERATION,
            Some("vm not found in qcvm_config"),
        ));

    }

}

/// Returns a vector of Vm Configs
pub fn parse_vm_config_json() -> Result<Vec<VmConfig>>{
    let mut vm_configs = Vec::<VmConfig>::new();

    // Uses serde_json to deserialize directly into strongly typed VmConfig object.
    println!("VM config path {:?}", VM_CONFIG_PATH);
    let config_file = File::open(VM_CONFIG_PATH)?;
    println!("Config File opened successfully");
    let root: Value = match serde_json::from_reader(config_file){
        Ok(parsed) => parsed,
        Err(e) => {
                error!("Parsing of JSON file is incorrect : {}",e);
                return Err(e.into());
            }
    };
    println!("Json has been parsed");
    let json_config_array: &Vec<Value> = to_binder_result(root
        .get("qcvm_config")
        .and_then(|mgr| mgr.get("vm_configs"))
        .and_then(|arr| arr.as_array())
        .ok_or("VM Configuration is invalid."))?;
    for config in json_config_array {
        let slot_suffix = system_properties::read("ro.boot.slot_suffix")
         .context("Failed to read ro.boot.slot_suffix")?
         .ok_or_else(|| anyhow!("slot_suffix is none"))?;
        match serde_json::from_value::<VmConfig>(config.to_owned()) {
            Ok(mut vm_config) => {
                if vm_config.name == "trustedvm"{
                    vm_config.vm_id = 45;
                    vm_config.pas_id = 28;
                    if vm_config.cma_size == 0 {
                        vm_config.cma_size = 68;
                    }
                    if vm_config.swiotlb_size == 0 {
                        vm_config.swiotlb_size = 16;
                    }
                    vm_config.total_memory= vm_config.cma_size + vm_config.swiotlb_size;
                    if vm_config.vm_dtbo_path == String::from("") {
                        vm_config.vm_dtbo_path = format!("/dev/block/by-name/qtvm_dtbo{}",slot_suffix)
                    }
                }
                //Assume it is a type of oemvm
                else {
                    vm_config.vm_id = 49;
                    vm_config.pas_id = 34;
                    if vm_config.cma_size == 0 {
                        vm_config.cma_size = 76;
                    }
                    if vm_config.swiotlb_size == 0 {
                        vm_config.swiotlb_size = 16;
                    }
                    vm_config.total_memory= vm_config.cma_size + vm_config.swiotlb_size;
                    if vm_config.vm_dtbo_path == String::from("") {
                        vm_config.vm_dtbo_path = format!("/dev/block/by-name/qtvm_dtbo{}",slot_suffix)
                    }
                }
                info!("VM Config: {:?}", vm_config);
                vm_configs.push(vm_config);
            }
            Err(e) => {
                // Just skip any malformed vm config.
                let name: &str = config
                    .get("name")
                    .and_then(|val| val.as_str())
                    .unwrap_or("No Name Specified");
                error!("Skipping entry for '{}'. Err: {e}", name);
                continue;
            }
        }
    }
    Ok(vm_configs)
}

/// Binder requires results to have a Binder Status, move the error handling
/// outside of HAL calls into other functions as much as possible.
/// Converts results to Binder results that implement Binder Status
pub fn to_binder_result<T, E: Debug>(result: Result<T, E>) -> BinderResult<T> {
    result.map_err(|e| {
        let message = format!("{:?}", e);
        warn!("Returning binder error: {}", &message);
        Status::new_exception_str(ExceptionCode::UNSUPPORTED_OPERATION, Some(message))
    })
}


/// Cannot clone VmInfo since it is an AIDL interface, need to create a custom
/// cloner.
pub fn clone_vm_infos(vm_infos: &Vec<VmInfo>) -> Result<Vec<VmInfo>>{
    let mut clone_infos = Vec::<VmInfo>::new();
    for vm_info in vm_infos {
        let clone_info = VmInfo{
            name: vm_info.name.clone(),
            early_vm: vm_info.early_vm.clone(),
            enabled: vm_info.enabled.clone(),
            force_stop: vm_info.force_stop.clone(),
            num_vcpus: vm_info.num_vcpus.clone().try_into()?,
            cma_size: vm_info.cma_size.clone().try_into()?,
            swiotlb_size: vm_info.swiotlb_size.clone().try_into()?,
            total_memory: vm_info.total_memory.clone().try_into()?,
            vm_id: vm_info.vm_id.clone().try_into()?,
            mink_uid: vm_info.mink_uid.clone().try_into()?,
        };
        clone_infos.push(clone_info);

    }
    Ok(clone_infos)

}
