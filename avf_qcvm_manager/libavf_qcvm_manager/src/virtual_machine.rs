/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use binder::{
    BinderFeatures, Strong, Result as BinderResult, Interface,
    DeathRecipient, IBinder
};

use vmm_sys_util::ioctl_ioc_nr;
use vmm_sys_util::ioctl_io_nr;
use vmm_sys_util::ioctl::ioctl_with_val;
use vmm_sys_util::errno::errno_result;

use serde::Deserialize;
use std::thread;
use std::{sync::{Arc, Mutex, Weak}, fmt::Debug};
use std::ffi::CString;
use rustutils::system_properties;
use nix::sys::stat::fstat;
use std::os::raw::c_ulong;
use anyhow::{anyhow, Context, Result};
use log::{info, debug, error, warn};
use std::fs::{File, OpenOptions, remove_file};
use std::os::fd::IntoRawFd;
use std::os::fd::RawFd;
use std::os::fd::OwnedFd;
use std::os::unix::io::FromRawFd;

use qcomvendor_libavf_bindgen::{AVirtualMachine_createRaw,
    AVirtualMachineRawConfig_setHypervisorSpecificAuthMethod, AVirtualMachineRawConfig_setInstanceId,
    AVirtualMachineRawConfig_setVCpuCount, AVirtualMachineRawConfig_setSwiotlbMiB,
    AVirtualMachineRawConfig_addDisk, AVirtualMachineRawConfig_setMemoryMiB,
    AVirtualMachineRawConfig_setProtectedVm, AVirtualMachineRawConfig_setKernel,
    AVirtualMachineRawConfig_setName, AVirtualMachineRawConfig_create, AVirtualMachine,
    AVirtualizationService, AVirtualizationService_destroy, AVirtualMachine_waitForStop,
    AVirtualMachineStopReason, AVirtualMachine_start, AVirtualMachine_stop,
    AVirtualMachine_destroy, AVirtualMachineRawConfig,AVirtualMachineRawConfig_addCustomMemoryBackingFile,
    AVirtualizationService_create, AVirtualMachineRawConfig_setDeviceTreeOverlay};

use vendor_qti_AvfQcvmManager::aidl::vendor::qti::AvfQcvmManager::{
    VmInfo::VmInfo, IVirtualMachine::{IVirtualMachine, BnVirtualMachine},
    IVirtualMachineCallback::IVirtualMachineCallback, VirtualMachineError::VirtualMachineError,
};

use crate::dtbo::*;
use guest_agent_client::{ServiceId, GuestAgentClient, IGuestNotificationCallback};
use crate::guest_client::*;
use crate::to_binder_result;

const GH_ANDROID_IOCTL_TYPE: u8 = 65u8;
const CMA_TUI_VM: &str = "/dev/trustedvm_cma";
const CMA_OEM_VM: &str = "/dev/oemvm_cma";
const VM_USERSPACE_RETRY: u32 = 3;
static DEFAULT_VM_START_TIMER: u32 = 2500;   // Time in milliseconds to wait before trying to connect to Mink hub to accommodate VM bootup time
static DEFAULT_USERSPACE_TIMER: u32 = 5000; // Time in milliseconds to wait between successive trials to connect to VM shutdown service
static DEFAULT_FORCE_SHUTDOWN: bool = false;
static DEFAULT_EARLY_VM: bool = true;

ioctl_io_nr!(GH_ANDROID_CREATE_CMA_MEM_FD, GH_ANDROID_IOCTL_TYPE.into(), 0x14);

fn default_force_shutdown() -> bool {
    DEFAULT_FORCE_SHUTDOWN
}

fn default_early_vm() -> bool {
    DEFAULT_EARLY_VM
}

fn default_userspace_timer() -> u32{
    DEFAULT_USERSPACE_TIMER
}

 fn default_vm_start_timer_timeout() -> u32{
     DEFAULT_VM_START_TIMER
 }

 fn default_userspace_retry() -> u32{
     VM_USERSPACE_RETRY
 }
/// VmClient holds relevant information about the Client
/// as well as a handle to their Callback and to their DeathRecipient
#[derive(Default)]
pub struct VmClient {
    /** VirtualMachineCallback doesn't implment Default, but wrapping in Option
     * defaults to None */
    pub vm_client_callback: Option<Strong<dyn IVirtualMachineCallback>>,
    /** Death Recipient doesn't implment Default, but wrapping in Option does */
    pub death_recipient: Option<DeathRecipient>,

}

/// This Uevent may no longer be needed since AVF returns error callbacks from
/// Crosvm during the wait_for_stop() thread
#[derive(Default, Debug, Clone, Deserialize, Eq, Hash, PartialEq)]
pub struct UeventInfo {
    pub vm_name: String,
    pub event: String,
    pub event_reason: u32,
}


/// DiskProperties helps capsulize the information for a Disk
#[derive(Default, Debug, Clone, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename = "disk")]
pub struct DiskProperties {
    pub image: String,
    pub label: u32,
    pub read_write: bool,
}

/// The Config JSON will be used to fill in this struct for all possible VM
/// configuration. Certain configurations are not client configurable like vm_id
/// and pas_id. Also CMA size is not configurable for this release
#[derive(Default, Debug, Clone, Deserialize, Eq, Hash, PartialEq)]
pub struct VmConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub disk: Vec<DiskProperties>,
    #[serde(default)]
    pub kernel: String,
    #[serde(default = "default_early_vm")]
    pub early_vm: bool,
    #[serde(default)]
    pub total_memory: u64,
    #[serde(default)]
    pub swiotlb_size: u64,
    #[serde(default)]
    pub cma_size: u64,
    // #[serde(default)]
    // pub cid: u64,
    #[serde(default)]
    pub vsock_port: u32,
    #[serde(default = "default_force_shutdown")]
    pub force_stop: bool,
    #[serde(default = "default_userspace_retry")]
    pub vm_userspace_retry_count: u32,
    #[serde(default = "default_vm_start_timer_timeout")]
    pub vm_start_timer: u32,
    #[serde(default = "default_userspace_timer")]
    pub userspace_timer: u32,
    #[serde(default)]
    pub mink_uid: u32,
    #[serde(default)]
    pub num_vcpus: u32,
    #[serde(default)]
    pub vm_id: u16,
    #[serde(default)]
    pub pas_id: u32,


    /// What if OEMs want to make their own VM DTBO partition???
    /// We make a VM DTOB partition in LE workspace but OEM VM doesn't use LE for some OEMs
    /// They will probably have to make their own vm dtbo just for OEM VM
    #[serde(default)]
    pub vm_dtbo_path: String,


}

/// Enum to represent the different states of the VM
/// Only has one strong reference object in VirtualMachine
#[derive(Debug, Clone, Copy)]
pub enum State{
    Stopped,
    Started,
    UserspaceReady,
    ShuttingDown,
    Crashed,
}

/// This is a wrapper around the *mut AVirtualMachine
/// A wrapper is needed because the *mut AVirtualMachine cannot be shared between
/// threads safely, so wrap it in a Struct that implements Send unsafely.
/// This is okay because it is locked via an Arc<Mutex<>>
#[derive(Default, Debug)]
pub struct AVirtualMachineWrapper(Option<*mut AVirtualMachine>);
unsafe impl Send for AVirtualMachineWrapper {}

/// This is a wrapper around the *mut AVirtualizationService.
/// This wrapper is needed to help share the C pointer between threads safely.
/// It will be safe as long as it is wrapped with an Arc<Mutex<>>
#[derive(Default, Debug)]
pub struct AVirtmgrWrapper(Option<*mut AVirtualizationService>);
unsafe impl Send for AVirtmgrWrapper {}


/// This struct is to represent the handles to AVF unsafe objects
/// Since these are raw C pointers, to share between thread, need to implement
/// Send unsafely.
#[derive(Default, Debug)]
pub struct AvfHandle {
    pub avf_vm_handle: Arc<Mutex<AVirtualMachineWrapper>>,
    pub avf_config: Option<*mut AVirtualMachineRawConfig>,
    pub virtmgr_service: Arc<Mutex<AVirtmgrWrapper>>,
}

unsafe impl Send for AvfHandle {}

/// Grouping all guest agent / userspace connection related members together
#[derive(Default)]
pub struct GuestManager{
    /** This is an object to hold a reference for the shutdown Mink Service */
    pub guest_client: Arc<Mutex<Option<GuestClient>>>,
    /** Callback object to be passed to guest agent so as to get reboot handle
        lifecycle notifications */
    pub guest_callback: Arc<Mutex<Option<Arc<Mutex<GuestNotificationCallback>>>>>,
    /** Thread to monitor reboot handle to know when to retry userspace connection */
    pub guest_monitor_thread: Arc<Mutex<Option<thread::JoinHandle<Result<()>>>>>,
}

/// Since VirtualMachine Struct gets consumed via to_binder(), this struct is
/// used to handle the VM with Default() and thread safe.
#[derive(Default)]
pub struct VmInstance{
    /** To be deprecated once VmInfo can host all of VmConfig */
    pub vm_config: VmConfig,
    /** Container for AVF pointers */
    pub avf_handle: Arc<Mutex<AvfHandle>>,
    /** Needs a Mutex since the Clients get impacts on DeathRecipient threads
     * and Stop threads */
    pub vm_clients: Arc<Mutex<Vec<VmClient>>>,
    /** Don't remake the vm_dtbo, hold a reference */
    pub vm_dtbo_fd: Option<RawFd>,
    /** Weak mutex, doesn't need to be strong since VirtualMachine holds a
     * strong reference */
    pub state: Weak<Mutex<State>>,
    /** Need a thread to wait until the VM stops when clients request for a stop */
    pub wait_for_stop_thread: Arc<Mutex<Option<thread::JoinHandle<Result<()>>>>>,
    /** Wait till the Userspace is ready */
    pub wait_for_userspace_thread: Arc<Mutex<Option<thread::JoinHandle<Result<()>>>>>,
    /** Handler for guest agent / userspace connection */
    pub guest_manager: GuestManager,
}

impl VmInstance {

    /// This start_vm call is what will call the necessary AVF APIs in order
    /// to boot the VM.
    pub fn start_vm(&mut self) -> Result<()>{

        {
            let virtmgr_service_lock = Arc::clone(&self.avf_handle.lock().unwrap().virtmgr_service);
            let virtmgr = virtmgr_service_lock.lock().unwrap();
            match virtmgr.0 {
                Some(service) => {
                    if service.is_null() {
                        //Create an instance of Virtmgr
                        self.get_virtmgr()?;
                    }

                },
                None => {
                    //Create an instance of Virtmgr
                    self.get_virtmgr()?;
                }
            }
        }

        // Set up the AVF Config -> this object dies when the VM dies
        self.create_avf_config()?;

        let virtmgr_service_lock = Arc::clone(&self.avf_handle.lock().unwrap().virtmgr_service);
        let virtmgr = virtmgr_service_lock.lock().unwrap();
        let mut vm = std::ptr::null_mut();
        let res = unsafe {
            // Create an AVF Virtual Machine object
            AVirtualMachine_createRaw(
                virtmgr.0.unwrap(), self.avf_handle.lock().unwrap().avf_config.unwrap(), -1, // console_in
                -1, // console_out
                -1, // log
                &mut vm,
            )
        };
        drop(virtmgr);

        if res != 0 {
            info!("we have failed to make a VM");
            return errno_result()?;
        }
        info!("Created AVF VM instance Successfully {:?}", vm);

        unsafe {
            AVirtualMachine_start(vm);
        }
        info!("Crosvm Launched!");

        self.avf_handle.lock().unwrap().avf_vm_handle = Arc::new(Mutex::new(AVirtualMachineWrapper(Some(vm))));

        // Start the wait for userspace thread
        self.wait_for_userspace()?;

        // Set up the stop thread to wait for stop immediately in case of crashes
        self.wait_for_stop()?;

        Ok(())

    }

    /// This method is to create an instance of virtmgr. It needs to be recreated
    /// each time because the type of virtmgr depends on the VM. A VM can use
    /// either Early Virtmgr which requires an xml but allows boot before post-fs
    /// and after boot complete. Or use Virtmgr which doesn't require an xml but
    /// must be used after boot complete.
    fn get_virtmgr(&mut self) -> Result<()>{
        let mut service = std::ptr::null_mut();

        if self.vm_config.early_vm == true{

            // SAFETY: &mut service is a valid pointer to *AVirtualizationService
            let res = unsafe { AVirtualizationService_create(&mut service, true) };
            if res != 0 {
                return Err(anyhow!("AVirtualizationService_create failed for Early Virtmgr"));
            }

            info!("Early Virtmgr started! {:?}", service);
        }
        else{
            let res = unsafe { AVirtualizationService_create(&mut service, false) };
            if res != 0 {
                return Err(anyhow!("AVirtualizationService_create failed for Virtmgr"));
            }

            info!("Virtmgr started! {:?}", service);
        }

        self.avf_handle.lock().unwrap().virtmgr_service = Arc::new(Mutex::new(AVirtmgrWrapper(Some(service))));
        Ok(())
    }

    /// Sets up all the VM config for AVF to pass to Crosvm
    pub fn create_avf_config(&mut self) -> Result<()>{

        //Creating AVF Config
        let config = unsafe { AVirtualMachineRawConfig_create() };
        debug!("raw config created");
        let cstring = CString::new(self.vm_config.name.clone().as_str())?;
        unsafe{AVirtualMachineRawConfig_setName(config, cstring.as_ptr());}

        //Set the vcpu count
        unsafe{AVirtualMachineRawConfig_setVCpuCount(config, self.vm_config.num_vcpus as i32);}

        //We only support Protected VMs
        unsafe{AVirtualMachineRawConfig_setProtectedVm(config, true);}

        //We use a Gunyah Specific Authentication
        unsafe{AVirtualMachineRawConfig_setHypervisorSpecificAuthMethod(config, true);}

        //Set SWIOTOLB
        unsafe{AVirtualMachineRawConfig_setSwiotlbMiB(config, self.vm_config.swiotlb_size as i32);}

        //Encode the authentication in the instance ID
        /* Decode
        Vm ID = 45, Pass ID = 28
        let vm_id = u32::from_le_bytes(config.instance_id[60..64].try_into().unwrap());
        let pas_id = u16::from_le_bytes(config.instance_id[58..60].try_into().unwrap());
         */
        let mut instance_id: [u8; 64] = [0; 64];
        let pas_id_bytes = self.vm_config.pas_id.to_le_bytes();
        instance_id[60..64].copy_from_slice(&pas_id_bytes);
        let vm_id_bytes = self.vm_config.vm_id.to_le_bytes();
        instance_id[58..60].copy_from_slice(&vm_id_bytes);
        //Vendor initiated VMs must have instance_id starting with 0xFFFFFFFF
        let prefix: [u8; 4] = u32::MAX.to_le_bytes();
        instance_id[0..4].copy_from_slice(&prefix);
        info!("instance ID = {:?}", instance_id);
        unsafe{AVirtualMachineRawConfig_setInstanceId(config, &instance_id as *const u8, 64);}

        //Creating Kernel FD
        let kernel_file =
            File::open(&self.vm_config.kernel).expect("Failed to open kernel file");
        let kernel_fd = kernel_file.into_raw_fd();
        debug!("Kernel FD created! {:?}",kernel_fd);
        unsafe{AVirtualMachineRawConfig_setKernel(config, kernel_fd);}

        //Adding the disk images
        for disk in &self.vm_config.disk{
            if disk.read_write == false {
                let read_disk_image =  File::open(disk.image.clone()).expect("Failed to open Disk Image");
                let disk_image_fd = read_disk_image.into_raw_fd();
                info!("Disk FD created! for {:?}",disk.image);
                unsafe{AVirtualMachineRawConfig_addDisk(config, disk_image_fd, disk.read_write);}
            }
            else{
                let write_disk_image = OpenOptions::new().read(true).write(true).open(disk.image.clone())?;
                let disk_image_fd = write_disk_image.into_raw_fd();
                info!("Disk FD created! for {:?}",disk.image);
                unsafe{AVirtualMachineRawConfig_addDisk(config, disk_image_fd, disk.read_write);}
            }

        }

        //Creating CMA FD
        info!("CMA FD VM name = {:?}", &self.vm_config.name);
        let dev_node = match self.vm_config.name.as_str() {
            "trustedvm" =>  {
                debug!("trustedvm CMA");
                Some(File::open(CMA_TUI_VM).expect("Failed to open dev node file"))
            },
            _ => {
                debug!("oemvm CMA");
                Some(File::open(CMA_OEM_VM).expect("Failed to open dev node file"))
            }
        };

        let dev_node_file = dev_node.unwrap();
        let dev_node_fd = dev_node_file.into_raw_fd();
        let ref_dev_node = unsafe { OwnedFd::from_raw_fd(dev_node_fd) };
        let cma_fd = unsafe { ioctl_with_val(&ref_dev_node, GH_ANDROID_CREATE_CMA_MEM_FD(), 0 as c_ulong) };
        let start_addr: u64 = 0x80000000;
        let mut size: u64 = fstat(cma_fd).unwrap().st_size as u64;
        info!("Max CMA Size = {:?}, ", size);
        if (self.vm_config.cma_size as u64)*1024*1024 <= size {
            size = self.vm_config.cma_size as u64 *1024*1024;
            info!("CMA size = {:?}, Fd = {:?}", size, cma_fd);
        }
        else {
            info!("CMA size is larger than its Carveout! Capping the size to the max")
        }
        let end_addr: u64 = start_addr + size;

        unsafe{AVirtualMachineRawConfig_addCustomMemoryBackingFile(config, cma_fd,
            start_addr,  end_addr);}
        // If we are debuggable, the total memory can also include private scattered mem
        let is_debuggable = system_properties::read("ro.debuggable")
         .context("Failed to read ro.debuggable")?
         .ok_or_else(|| anyhow!("ro.debuggable is none"))?;
        if is_debuggable == String::from("0") {
            self.vm_config.total_memory = (size / 1024 / 1024) + self.vm_config.swiotlb_size;
        }
        else{
            //If total memory is less than cma + shared, then set the private mem to be fully CMA size.
            if self.vm_config.total_memory < (size / 1024 / 1024) + self.vm_config.swiotlb_size {
                info!("Total mem is too low! it should be Private mem + Shared, setting it to CMA + Shared");
                self.vm_config.total_memory = (size / 1024 / 1024) + self.vm_config.swiotlb_size;
            }
        }

        //Set Private Mem (CMA + Scattered)
        unsafe{AVirtualMachineRawConfig_setMemoryMiB(config, self.vm_config.total_memory as i32 - self.vm_config.swiotlb_size as i32);}

        //Create a tmp VM Dtbo and set it
        let slot_suffix = system_properties::read("ro.boot.slot_suffix")
         .context("Failed to read ro.boot.slot_suffix")?
         .ok_or_else(|| anyhow!("slot_suffix is none"))?;
        self.get_vm_dtbo(format!("/dev/block/by-name/qtvm_dtbo{}",slot_suffix))?;

        unsafe{
            AVirtualMachineRawConfig_setDeviceTreeOverlay(config, self.vm_dtbo_fd.clone().unwrap());
        }

        self.avf_handle.lock().unwrap().avf_config = Some(config);
        Ok(())

    }

    /// Meant to be implemented once ABL sets up the ro board properties
    fn get_vm_dtbo_index(&self) -> Result<i32>{
        let name = self.vm_config.name.clone();
              match name.as_str(){
              "trustedvm" => {
                    let tuivm_sys_prop =  system_properties::read("ro.boot.hypervisor.tuivm_dtbo_idx")
                    .context("Failed to read vm_dtbo_idx")?
                    .ok_or_else(|| anyhow!("vm_dtbo_idx is none"))?;

                    let tuivm_idx: i32 = tuivm_sys_prop.parse().context("vm_dtbo_idx is not an integer")?;
                    info!("Tuivm Index: {tuivm_idx}");
                    return Ok(tuivm_idx);
                },
            _ =>  {
                    let oemvm_sys_prop =  system_properties::read("ro.boot.hypervisor.oemvm_dtbo_idx")
                    .context("Failed to read vm_dtbo_idx")?
                    .ok_or_else(|| anyhow!("vm_dtbo_idx is none"))?;

                    let oemvm_idx: i32 = oemvm_sys_prop.parse().context("vm_dtbo_idx is not an integer")?;
                    info!("oemvm Index: {oemvm_idx}");
                    return Ok(oemvm_idx);
                }
              };
    }

    /// Sets up the VM DTBO FD to pass to AVF. It will first copy the contents
    /// from the vm_dtbo.img  and place it in a tmp file. Then it will cache the FD
    /// and pass it to AVF.
    fn get_vm_dtbo(&mut self, path: String) -> Result<()>{
        let vm_name = self.vm_config.name.clone();
        let temp_path = get_or_create_common_dir()?.join(format!("{vm_name}.dtbo"));
        info!("DTBO Path: {:?}",path);
        info!("temp_path: {:?}", temp_path);

        let idx = self.get_vm_dtbo_index()?;
        let mut dtbo_img = File::open(path).context("Failed to open DTBO partition")?;
        let dt_table_header = get_dt_table_header(&mut dtbo_img)?;
        info!("dt_table_header: {:?}", dt_table_header);
        let dt_table_entry = get_dt_table_entry(
            &mut dtbo_img, &dt_table_header, idx as u32)?;
        info!("dt_table_entry: {:?}", dt_table_entry);

        if temp_path.exists() {
            // All temporary files are deleted when the service is started.
            // If the file exists but the FD is not cached, the file is
            // likely corrupted.
            remove_file(&temp_path).context("Failed to clone cached VM DTBO file descriptor")?;
        }
        let mut dtbo_tmp_file = File::create(&temp_path).context("Failed to create VM DTBO file")?;
        copy_vm_full_dtbo_from_img(
            &mut dtbo_img,
            &dt_table_entry,
            temp_path.clone(),
            &mut dtbo_tmp_file)?;
        let dtbo_tmp_file = File::open(&temp_path).context("Failed to create VM DTBO file")?;
        debug!("dtbo_file {:?}", dtbo_tmp_file);
        self.vm_dtbo_fd = Some(dtbo_tmp_file.into_raw_fd());
        debug!("dtbo raw fd = {:?}", self.vm_dtbo_fd.clone().unwrap());
        Ok(())
    }

    /// Notifies all registered clients about a change in the VM state
    pub fn notify_clients(vm_clients: &Vec<VmClient>, strong_state: State) -> Result<()>{
        info!("Notifying clients about update VM state: {:?}", strong_state);
        for client in vm_clients{
            let cb = client.vm_client_callback.clone().unwrap();
            match strong_state {
                State::Started => cb.onStarting()?,
                State::UserspaceReady => cb.onUserspaceReady()?,
                State::ShuttingDown => cb.onShutdownInitiated()?,
                State::Stopped => cb.onStopped()?,
                State::Crashed => cb.onCrashed()?,
            }
        }

        Ok(())
    }

pub fn wait_for_userspace(&mut self) -> Result<()>{
    let mink_uid = self.vm_config.mink_uid;
    let vsock_port = self.vm_config.vsock_port;

    if mink_uid == 0 && vsock_port == 0 {
        warn!("mink_uid and vsock_port are both not set in config, will not set userspace_ready and keep state as started and don't support request stop");
        return Ok(());
    }

    self.spawn_userspace_thread()?;
    self.monitor_userspace_handle()?;

    Ok(())
}

fn spawn_userspace_thread(&mut self) -> Result<()> {
    let vm_name = self.vm_config.name.clone();
    let userspace_timer = self.vm_config.userspace_timer;
    let vm_userspace_retry_count = self.vm_config.vm_userspace_retry_count;
    let vm_start_timer = self.vm_config.vm_start_timer;
    let mink_uid = self.vm_config.mink_uid;
    let vsock_port = self.vm_config.vsock_port;

    let strong_state = Weak::upgrade(&self.state).ok_or_else(|| anyhow!("State object has died"))?;
    let vm_clients_lock = Arc::clone(&self.vm_clients);
    let guest_client_lock = Arc::clone(&self.guest_manager.guest_client);
    let guest_callback_lock = Arc::clone(&self.guest_manager.guest_callback);

    let thread = thread::spawn(move|| -> Result<()>{
        info!("Connecting to {:?} userspace (mink_uid:{}, vsock_port:{})", vm_name, mink_uid, vsock_port);

        let service_id = if mink_uid != 0 {
            ServiceId::MinkUid(mink_uid)
        } else {
            ServiceId::VsockPort(vsock_port)
        };

        let guest_callback = Arc::new(Mutex::new(GuestNotificationCallback{ reboot_handle_available: None }));

        let guest_handle = GuestClient::connect_userspace(
            vm_userspace_retry_count,
            vm_start_timer,
            userspace_timer,
            service_id,
            Some(guest_callback.clone())
        ).map_err(|e| {
            let mut guest_client = guest_client_lock.lock().unwrap();
            *guest_client = None;
            anyhow!("Failed to connect to {:?} Guest Service: {:?}", vm_name, e)
        })?;

        info!("Successfully connected to {:?} userspace!", vm_name);

        // Store callback and client
        *guest_callback_lock.lock().unwrap() = Some(guest_callback);
        *guest_client_lock.lock().unwrap() = Some(guest_handle);

        // Update state and notify clients
        let mut state = strong_state.lock().unwrap();
        *state = State::UserspaceReady;
        let vm_clients = vm_clients_lock.lock().unwrap();
        VmInstance::notify_clients(&*vm_clients, *state)?;

        Ok(())
    });

    *self.wait_for_userspace_thread.lock().unwrap() = Some(thread);
    Ok(())
}

pub fn monitor_userspace_handle(&mut self) -> Result<()> {
    let vm_name = self.vm_config.name.clone();
    let userspace_timer = self.vm_config.userspace_timer;
    let vm_userspace_retry_count = self.vm_config.vm_userspace_retry_count;
    let vm_start_timer = self.vm_config.vm_start_timer;
    let mink_uid = self.vm_config.mink_uid;
    let vsock_port = self.vm_config.vsock_port;

    let state_lock = Weak::upgrade(&self.state).ok_or_else(|| anyhow!("State object has died"))?;
    let vm_clients_lock = Arc::clone(&self.vm_clients);
    let guest_callback_lock = Arc::clone(&self.guest_manager.guest_callback);
    let guest_client_lock = Arc::clone(&self.guest_manager.guest_client);

    let thread = thread::spawn(move|| -> Result<()> {
        info!("Started monitoring reboot handle for {:?}", vm_name);

        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Check if reboot handle is unavailable
            let handle_lost = {
                let callback_outer = guest_callback_lock.lock().unwrap();
                callback_outer.as_ref()
                    .and_then(|cb| cb.lock().ok())
                    .and_then(|cb| cb.get_reboot_handle_availability())
                    .map(|available| !available)
                    .unwrap_or(false)
            };

            if !handle_lost {
                continue;
            }

            info!("Reboot handle lost for {:?}, reconnecting to userspace", vm_name);

            // Clear old client and update state
            *guest_client_lock.lock().unwrap() = None;

            // Update state to Started and notify clients
            {
                let mut state = state_lock.lock().unwrap();
                *state = State::Started;
                let vm_clients = vm_clients_lock.lock().unwrap();
                if let Err(e) = VmInstance::notify_clients(&*vm_clients, *state) {
                    error!("Failed to notify clients about Started state: {:?}", e);
                }
            }

            // Reconnect to userspace
            let service_id = if mink_uid != 0 {
                ServiceId::MinkUid(mink_uid)
            } else {
                ServiceId::VsockPort(vsock_port)
            };

            let guest_callback = Arc::new(Mutex::new(GuestNotificationCallback{ reboot_handle_available: None }));

            match GuestClient::connect_userspace(
                vm_userspace_retry_count,
                vm_start_timer,
                userspace_timer,
                service_id,
                Some(guest_callback.clone())
            ) {
                Ok(guest_handle) => {
                    info!("Successfully reconnected to {:?} userspace", vm_name);

                    *guest_callback_lock.lock().unwrap() = Some(guest_callback);
                    *guest_client_lock.lock().unwrap() = Some(guest_handle);

                    // Update state to UserspaceReady and notify clients
                    {
                        let mut state = state_lock.lock().unwrap();
                        *state = State::UserspaceReady;
                        let vm_clients = vm_clients_lock.lock().unwrap();
                        if let Err(e) = VmInstance::notify_clients(&*vm_clients, *state) {
                            error!("Failed to notify clients about UserspaceReady state: {:?}", e);
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to reconnect to {:?} userspace: {:?}", vm_name, e);
                    // Continue monitoring, will retry on next iteration if handle is still lost
                }
            }
        }
    });

    *self.guest_manager.guest_monitor_thread.lock().unwrap() = Some(thread);
    Ok(())
}

    /// Creates a stop thread for the VM to get notified when the VM has stopped
    /// asynchronously. It will also notify all the clients about whether it stopped
    /// via a Crash or not.
    pub fn wait_for_stop(&mut self) -> Result<()>{
        let vm_lock = Arc::clone(&self.avf_handle.lock().unwrap().avf_vm_handle);
        let virtmgr_service_lock = Arc::clone(&self.avf_handle.lock().unwrap().virtmgr_service);
        let vm_name = self.vm_config.name.clone();
        let strong_state = Weak::upgrade(&self.state);
        let vm_clients_lock = Arc::clone(&self.vm_clients);
        let thread = thread::spawn(move|| -> Result<()>{
            info!("Entered Wait for stop thread for {:?}", vm_name);
            let vm_wrapper = vm_lock.lock().unwrap();
            let vm = vm_wrapper.0.unwrap().clone();
            drop(vm_wrapper);
            let mut virtmgr_wrapper = virtmgr_service_lock.lock().unwrap();
            let virtmgr = virtmgr_wrapper.0.unwrap().clone();
            virtmgr_wrapper.0 = None;
            drop(virtmgr_wrapper);
            let mut stop_reason = AVirtualMachineStopReason::AVIRTUAL_MACHINE_KILLED;
            info!("Waiting for {:?} to stop", vm_name);
            unsafe{AVirtualMachine_waitForStop(vm, std::ptr::null_mut(), &mut stop_reason)};
            info!("{:?} VM has stopped for {:?}", vm_name, stop_reason);
            // Destroy all handles to Virtmgr and the VM in AVF so Virtmgr can close
            unsafe{AVirtualMachine_destroy(vm);}
            unsafe{AVirtualizationService_destroy(virtmgr)};
            info!("{:?} Virtmgr has been destroyed", vm_name);
            if let Some(state_lock) = strong_state{
                let mut state = state_lock.lock().unwrap();
                if stop_reason == AVirtualMachineStopReason::AVIRTUAL_MACHINE_CRASH ||
                    stop_reason == AVirtualMachineStopReason::AVIRTUAL_MACHINE_START_FAILED {
                    *state = State::Crashed;
                    info!("{:?} VM has crashed", vm_name);
                }
                else{
                    *state = State::Stopped;
                    info!("{:?} has stopped successfully", vm_name);
                }
                let mut vm_clients = vm_clients_lock.lock().unwrap();
                Self::notify_clients( &*vm_clients, *state)?;
                let clients = &mut vm_clients;
                clients.clear();

                return Ok(());
            }
            Err(anyhow!("State object has died"))
        });

        self.wait_for_stop_thread = Arc::new(Mutex::new(Some(thread)));

        debug!("Stopped thread stored successfully");

        Ok(())
    }

    // pub fn stop_vm(force: bool, stop_service_handle:)

    /// This function is to determine if a client already is registered
    pub fn find_client(&self, cb: &Strong<dyn IVirtualMachineCallback>) -> bool{
        let vm_clients_lock = Arc::clone(&self.vm_clients);
        let vm_clients = vm_clients_lock.lock().unwrap();
        let found = vm_clients.iter().find(|client| client.vm_client_callback.as_ref().cloned().unwrap().eq(cb));
        if let None = found {
            return false;
        }
        true
    }
}

#[derive(Default)]
pub struct GuestNotificationCallback {
    reboot_handle_available: Option<bool>,
}

impl IGuestNotificationCallback for GuestNotificationCallback {
    fn set_reboot_handle_availability(&mut self, status: Option<bool>) -> Result<()> {
        self.reboot_handle_available = status;
        Ok(())
    }

    fn get_reboot_handle_availability(&self) -> Option<bool> {
        self.reboot_handle_available
    }
}

/// ===========================================================
/// CORE IMPLEMENTATION
/// ===========================================================
/// We need a wrapper around the AIDL since it consumes the object. It prevents
/// cloning and default, also leave the VmInfo here as well since its also an AIDL
/// interface.
#[derive(Clone)]
pub struct VirtualMachine{
    pub vm_instance: Arc<Mutex<VmInstance>>,
    pub main_state: Arc<Mutex<State>>,
}

impl VirtualMachine{
    pub fn to_binder(self) -> Strong<dyn IVirtualMachine> {
        BnVirtualMachine::new_binder(self, BinderFeatures::default())
    }

    pub fn new() -> Self{
        VirtualMachine{
            vm_instance: Default::default(),
            main_state: Arc::new(Mutex::new(State::Stopped)),
        }
    }

    /// This is to create an Instance of the Vm Client with binder callback
    /// It will create a death recipient as well to link to the callback
    pub fn create_client(&self, cb: &Strong<dyn IVirtualMachineCallback>) -> BinderResult<()>{
        let cb_clone = cb.clone();
        let death_lock = Arc::clone(&self.vm_instance);
        let vm_instance = death_lock.lock().unwrap();
        if vm_instance.find_client(&cb) == true {
            info!("This Client already registered");
            return Ok(());
        }
        let vm_name = vm_instance.vm_config.name.clone();
        let vm_clients_lock = Arc::clone(&vm_instance.vm_clients);
        let guest_client_lock = Arc::clone(&vm_instance.guest_manager.guest_client);
        let state_lock = Arc::clone(&self.main_state);
        let userspace_ready_thread_lock = Arc::clone(&vm_instance.wait_for_userspace_thread);
        let unsupported_request = vm_instance.vm_config.vsock_port == 0 && vm_instance.vm_config.mink_uid == 0;
        let avf_handle_lock = Arc::clone(&vm_instance.avf_handle);
        let mut death_recipient = DeathRecipient::new(move ||{
                info!("Received a death recipient");
                let mut vm_clients = vm_clients_lock.lock().unwrap();
                if let Some(idx) = vm_clients.iter().position(|client|
                    client.vm_client_callback.as_ref().cloned().unwrap().eq(&cb_clone))
                    {
                        info!(
                            "Cleared the callback object for {}!",
                            vm_name);
                        vm_clients.remove(idx);
                    }
                // Todo: Figure out why the Userspace ready thread shows up as none
                // The thread is clearly still running and hasn't joined because the state
                // is Started, if userspace joined it would've been UserspaceReady.
                // Since I know its not joined, the only option is it is not being set,
                // but if the state is Started, that means start_vm() exited successfully.
                // Final option, there is something that is taking and joining the thread
                // in between the calls.
                if vm_clients.len() == 0 {
                    if unsupported_request {
                        info!("All Clients for {:?} has disconnected, but unsupport request stop, force stop now", vm_name);
                        let wrapper = avf_handle_lock.lock().unwrap();
                        let vm = wrapper.avf_vm_handle.lock().unwrap().0.unwrap().clone();
                        //Removes the virtmgr instance and the VM to clean up
                        unsafe{AVirtualMachine_stop(vm);}
                    } else {
                        info!("All Clients for {:?} has disconnected, will try to request shutdown", vm_name);
                        //drop the clients handle,
                        drop(vm_clients);

                        {
                            let state = state_lock.lock().unwrap();
                            //Wait for userspace to connect first
                            if let State::Started = *state {
                                //The userspace thread needs this lock, drop it so that it can pick it up
                                drop(state);
                                let mut userspace_ready_thread = userspace_ready_thread_lock.lock().unwrap();
                                info!("Wait till userspace connects before shutting down");
                                //Wait to join the userspace ready thread.
                                if let Some(handle) = userspace_ready_thread.take() {
                                    match handle.join() {
                                        Ok(Ok(())) => info!("Userspace thread joined successfully"),
                                        Ok(Err(e)) => error!("Cannot request to shutdown, the VM Guest Service could not connect {:?} ", e),
                                        Err(e) => error!("Failed to join userspace thread: {:?}", e),
                                        }
                                } else {
                                    warn!("Userspace thread was already taken or joined alrady");
                                }
                            }
                        }
                        let mut state = state_lock.lock().unwrap();
                        let guest_client = guest_client_lock.lock().unwrap();
                        if let State::UserspaceReady = *state {
                            info!("Calling shutdown on the Guest Agent");
                            // There are no clients connected,
                            // no need to check when the shutdown thread joins
                            let _ = guest_client.as_ref().unwrap().shutdown();
                            *state = State::ShuttingDown;
                        }
                    }
                }
            });
        let mut cb_binder = cb.as_binder();
        cb_binder.link_to_death(&mut death_recipient)?;
        let vm_clients_lock = Arc::clone(&vm_instance.vm_clients);
        let mut vm_clients = vm_clients_lock.lock().unwrap();
        info!("Created a client instance! Adding to list of Clients");
        vm_clients.push(VmClient{
            vm_client_callback: Some(cb.clone()),
            death_recipient: Some(death_recipient),
        });
        Ok(())
    }
}

impl Interface for VirtualMachine{

}

impl IVirtualMachine for VirtualMachine{


    /// Return a VM Info for Client information. This contains a subset of the VM
    /// configuration
    fn getVmInfo(&self) -> BinderResult<VmInfo>{
        let vm_instance_lock = Arc::clone(&self.vm_instance);
        let vm_instance = vm_instance_lock.lock().unwrap();
        let vm_config = vm_instance.vm_config.clone();
        let vm_info = to_binder_result(create_vm_info(&vm_config))?;
        info!("VM Info {:?}", vm_info);
        Ok(vm_info)
    }

    /// Start the VM using AVF APIs
    /// Also register the client callback.
    /// If the VM has crashed, you can try starting but may require a restart.
    fn start(&self, _arg_callback: &Strong<dyn IVirtualMachineCallback>) -> BinderResult<()>{
        let cb = _arg_callback;
        self.create_client(cb)?;
        let state_lock = Arc::clone(&self.main_state);
        let mut state = state_lock.lock().unwrap();
        let vm_instance_lock = Arc::clone(&self.vm_instance);
        let mut vm_instance = vm_instance_lock.lock().unwrap();
            let _ = match *state {
                //The VM is already starting, just add the client and notify it.
                State::Started => {
                    info!("{:?} was starting, return back", vm_instance.vm_config.name);
                    let _  = cb.onStarting();
                },
                //The VM already started and in Userspace, just add the client
                //and notify it.
                State::UserspaceReady => {
                    info!("{:?} Usespace is already ready, return back", vm_instance.vm_config.name);
                    let _ = cb.onUserspaceReady();
                }
                //You can try to start, but may not always work.
                State::Crashed => {
                    cb.onCrashed()?;
                    info!("{:?} VM has crashed but will try to start the VM",vm_instance.vm_config.name);
                    match vm_instance.start_vm(){
                        Ok(()) => {
                            info!("{:?} VM is starting!", vm_instance.vm_config.name);
                            *state = State::Started;
                            let vm_clients_lock = Arc::clone(&vm_instance.vm_clients);
                            let vm_clients= vm_clients_lock.lock().unwrap();
                            to_binder_result(VmInstance::notify_clients(&*vm_clients,state.clone()))?;
                        }
                        Err(e) => {
                            error!("{:?} VM failed to start, reason: {:?}", vm_instance.vm_config.name, e);
                            let _ = cb.onError(VirtualMachineError::FAILED_START);
                            return Ok(());
                        }
                    };

                },
                //This shouldn't be hit, but in case, the client should wait until they received an onStopped()
                State::ShuttingDown => {
                    info!("{:?} VM is shutting down call start after the VM has stopped",vm_instance.vm_config.name);
                    let _ = cb.onShutdownInitiated();
                },

                //Add the client, notify you are starting the VM. Then try starting
                //the vm.
                State::Stopped => {
                    info!("VM was stopped, needs to start");
                    match vm_instance.start_vm(){
                        Ok(()) => {
                            info!("{:?} VM is starting!", vm_instance.vm_config.name);
                            *state = State::Started;
                            let vm_clients_lock = Arc::clone(&vm_instance.vm_clients);
                            let vm_clients= vm_clients_lock.lock().unwrap();
                            to_binder_result(VmInstance::notify_clients(&*vm_clients,state.clone()))?;
                        }
                        Err(e) => {
                            error!("{:?} VM failed to start, reason: {:?}", vm_instance.vm_config.name, e);
                            let _ = cb.onError(VirtualMachineError::FAILED_START);
                            return Ok(());
                        }
                    };
                },
            };
        info!("state has been dropped");
        Ok(())
    }

    /// Stop the VM forcibly. Using the AVF APIs, call destroy and clean up the
    /// AVF objects. Wait until the stop thread joins.
    fn stop(&self, _arg_callback: &Strong<dyn IVirtualMachineCallback>) -> BinderResult<()>{
        let cb = _arg_callback.clone();
        let vm_instance_lock = Arc::clone(&self.vm_instance);
        let vm_instance = vm_instance_lock.lock().unwrap();
        if vm_instance.vm_config.force_stop == false {
            let _ = cb.onError(VirtualMachineError::FAILED_STOP);
            error!("Force kill is not enabled for {:?}, try request_stop()", vm_instance.vm_config.name);
            return Ok(());
        }
        if vm_instance.find_client(&cb) == false {
            error!("This Client is not registered! Clients need to register with start() first");
            let _ = cb.onError(VirtualMachineError::FAILED_STOP);
            return Ok(());
        }
        let state_lock = Arc::clone(&self.main_state);
        let mut state = state_lock.lock().unwrap();
        let _ = match *state{
            State::Started | State::UserspaceReady => {
                info!("VM State is {:?}", state);
                *state = State::ShuttingDown;
                info!("{:?} VM is shutting down!", vm_instance.vm_config.name);
                let vm_client_lock = Arc::clone(&vm_instance.vm_clients);
                let vm_clients = vm_client_lock.lock().unwrap();
                to_binder_result(VmInstance::notify_clients(&*vm_clients, state.clone()))?;
                //Don't want to hold this mutex, stop thread will need need it
                drop(vm_clients);
                let wrapper = Arc::clone(&vm_instance.avf_handle.lock().unwrap().avf_vm_handle);
                let vm = wrapper.lock().unwrap().0.unwrap().clone();
                //Removes the virtmgr instance and the VM to clean up
                unsafe{AVirtualMachine_stop(vm);}
                info!("Called stop on {:?}", vm_instance.vm_config.name);

                //Drop the mutexs' here so that the stop thread can use them
                drop(state);
                drop(wrapper);
                let shutdown_thread_lock = Arc::clone(&vm_instance.wait_for_stop_thread);
                let mut shutdown_thread = shutdown_thread_lock.lock().unwrap();
                //Wait to join the stop thread.
                let thread_res = to_binder_result(shutdown_thread.take().unwrap().join())?;
                to_binder_result(thread_res)?;
            },
            State::ShuttingDown => {
                info!("{:?} is already shutting down", vm_instance.vm_config.name);
                let _ = cb.onShutdownInitiated();
            },
            State::Stopped => {
                info!("{:?} is already stopped", vm_instance.vm_config.name);
                let _ = cb.onStopped();

            },
            State::Crashed => {
                info!("{:?} is crashed, effectively stopped", vm_instance.vm_config.name);
                let _ = cb.onCrashed();
            },
        };
        Ok(())
    }

    /// The client requested to stop, but the VM is the deciding entity. Needs
    /// either a Mink based or Vsock connection. Will only shutdown once all clients unregister
    /// It is assumed that this client no longer needs the VM. Thus we will remove
    /// it's callback.
    fn request_stop(&self, _arg_callback: &Strong<dyn IVirtualMachineCallback>) -> BinderResult<()>{
        debug!("requesting VM to stop");
        let cb = _arg_callback.clone();
        let vm_instance_lock = Arc::clone(&self.vm_instance);
        let vm_instance = vm_instance_lock.lock().unwrap();
        if vm_instance.vm_config.vsock_port == 0 && vm_instance.vm_config.mink_uid == 0 {
            info!("mink_uid and vsock_port are both not set, unsupport request stop");
            let _ = cb.onError(VirtualMachineError::FAILED_TO_REQUEST_STOP);
            return Ok(());
        }
        if vm_instance.find_client(&cb) == false {
            info!("This Client is not registered! Clients need to register with start() first");
            let _ = cb.onError(VirtualMachineError::FAILED_TO_REQUEST_STOP);
            return Ok(());
        }
        let state_lock = Arc::clone(&self.main_state);
        //Creating a scope to determine if Userspace is ready.
        {
            let state = state_lock.lock().unwrap();
            //Wait for userspace to connect first
            if let State::Started = *state {
                //The userspace thread needs this lock, drop it so that it can pick it up
                drop(state);
                let userspace_ready_thread_lock = Arc::clone(&vm_instance.wait_for_userspace_thread);
                let mut userspace_ready_thread = userspace_ready_thread_lock.lock().unwrap();
                //Wait to join the userspace ready thread.
                if let Some(handle) = userspace_ready_thread.take() {
                    match handle.join() {
                        Ok(Ok(())) => info!("Userspace thread joined successfully"),
                        Ok(Err(e)) => error!("Cannot request to shutdown, the VM Guest Service could not connect {:?} ", e),
                        Err(e) => {
                                error!("Cannot request to shutdown, the VM Guest Service could not connect {:?} ", e);
                                let _ = cb.onError(VirtualMachineError::FAILED_TO_REQUEST_STOP);
                                return Ok(());
                            },
                        }
                } else {
                    warn!("Userspace thread was already taken or never started or exited");
                }
            }
        }

        let mut state = state_lock.lock().unwrap();
        match *state{
            State::Started => {
                info!("{:?} is starting then the userspace couldn't connect therefore VM is stuck", vm_instance.vm_config.name);
                let _ = cb.onError(VirtualMachineError::FAILED_TO_REQUEST_STOP);
            },
            State::UserspaceReady => {
                info!("{:?} Userspace Ready, request to shutdown -> removing callback ", vm_instance.vm_config.name);
                let vm_clients_lock = Arc::clone(&vm_instance.vm_clients);
                let mut vm_clients = vm_clients_lock.lock().unwrap();
                if let Some(idx) = vm_clients.iter().position(|client|
                    client.vm_client_callback.as_ref().cloned().unwrap().eq(&cb)) {
                        info!("Cleared the callback object!",);
                        vm_clients.remove(idx);
                }

                if vm_clients.len() == 0 {
                    let guest_client_lock = Arc::clone(&vm_instance.guest_manager.guest_client);
                    //Safe to assume there is a Guest Service since the userspace
                    // thread joined successfully.
                    let guest_client = guest_client_lock.lock().unwrap();
                    match guest_client.as_ref().unwrap().shutdown() {
                        Ok(()) => {
                            info!("{} has started to shutdown",vm_instance.vm_config.name);
                            *state = State::ShuttingDown;
                            let _ = cb.onShutdownInitiated();
                            drop(state);
                            //drop the clients handle, they will all be removed in the stop thread
                            drop(vm_clients);
                            let shutdown_thread_lock = Arc::clone(&vm_instance.wait_for_stop_thread);
                            let mut shutdown_thread = shutdown_thread_lock.lock().unwrap();
                            //Wait to join the stop thread.
                            let thread_res = to_binder_result(shutdown_thread.take().unwrap().join())?;
                            if let Err(_e) = thread_res{
                                error!("Failed to request stop for {:?}", vm_instance.vm_config.name);
                                let mut state = state_lock.lock().unwrap();
                                *state = State::UserspaceReady;
                                let _ = cb.onError(VirtualMachineError::FAILED_TO_REQUEST_STOP);
                            }
                            else{

                                let _ = cb.onStopped()?;
                            }
                        }
                        Err(e) => {
                            error!("{} VM has failed to shutdown: {:?}", vm_instance.vm_config.name, e);
                            let _ = cb.onError(VirtualMachineError::FAILED_TO_REQUEST_STOP);
                            return Ok(());
                        }
                    };

                }
            },
            State::ShuttingDown => {
                info!("{:?} is already shutting down", vm_instance.vm_config.name);
                let _ = cb.onShutdownInitiated();
            },
            State::Stopped => {
                info!("{:?} has already stopped", vm_instance.vm_config.name);
                let _ = cb.onStopped();
            },
            State::Crashed => {
                info!("{:?} is crashed, effectively stopped", vm_instance.vm_config.name);
                let _ = cb.onCrashed();
            },

        };

        Ok(())
    }
}

/// Create a VM Info from a VmConfig
pub fn create_vm_info(vm_config: &VmConfig) -> Result<VmInfo>{
    let vm_info = VmInfo{
        name: vm_config.name.clone(),
        early_vm: vm_config.early_vm.clone(),
        enabled: vm_config.enabled.clone(),
        force_stop: vm_config.force_stop.clone(),
        num_vcpus: vm_config.num_vcpus.clone().try_into()?,
        cma_size: vm_config.cma_size.clone().try_into()?,
        swiotlb_size: vm_config.swiotlb_size.clone().try_into()?,
        total_memory: vm_config.total_memory.clone().try_into()?,
        vm_id: vm_config.vm_id.clone().try_into()?,
        mink_uid: vm_config.mink_uid.clone().try_into()?,
    };
    debug!("VM Info {:?}", vm_info);
    Ok(vm_info)
}
