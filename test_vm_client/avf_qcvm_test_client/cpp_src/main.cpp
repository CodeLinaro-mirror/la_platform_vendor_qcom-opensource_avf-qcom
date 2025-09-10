/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

#include <android/binder_manager.h>
#include <android/binder_process.h>
#include "VirtualMachineCallback.h"
#include <aidl/vendor/qti/AvfQcvmManager/IAvfQcvmManager.h>
#include <aidl/vendor/qti/AvfQcvmManager/IVirtualMachine.h>
#include <iostream>
#include <memory>
#include <thread>
#include <chrono>
#include <vector>

using namespace aidl::vendor::qti::AvfQcvmManager;
using namespace std;

class StateMachine {
public:
    StateMachine(const std::shared_ptr<IAvfQcvmManager>& service)
        : service_(service), vm_(nullptr), vm_name_(""),
        vm_state_(std::make_shared<std::atomic<VmState>>(VmState::NotStarted)),
        vm_callback_(nullptr) {}

    const char* to_string(VmState state) {
        switch (state) {
            case VmState::NotStarted: return "NotStarted";
            case VmState::Started: return "Started";
            case VmState::UserspaceReady: return "UserspaceReady";
            case VmState::ShutdownInitiated: return "ShutdownInitiated";
            case VmState::Crashed: return "Crashed";
            case VmState::Stopped: return "Stopped";
            case VmState::Error: return "Error";
            default: return "Unknown";
        }
    }

    void listAvailableVMs() {
        std::vector<VmInfo> vm_list;
        if (service_->availableVms(&vm_list).isOk()) {
            if (vm_list.empty()) {
                std::cout << "No available VMs found." << std::endl;
                return;
            }
            std::cout << "\nAvailable VMs:\n";
            for (size_t i = 0; i < vm_list.size(); ++i) {
                std::cout << i + 1 << ". " << vm_list[i].name << std::endl;
            }
        } else {
            std::cout << "Failed to retrieve available VMs." << std::endl;
        }
    }

    void getVm() {
        listAvailableVMs();

        std::cout << "Select a VM by number: ";
        int choice;
        std::cin >> choice;
        std::cin.ignore();

        std::vector<VmInfo> vm_list;
        if (service_->availableVms(&vm_list).isOk()) {
            if (choice > 0 && choice <= static_cast<int>(vm_list.size())) {
                vm_name_ = vm_list[choice - 1].name;
                service_->getVm(vm_name_, &vm_);
                if (vm_) {
                    vm_callback_ = ndk::SharedRefBase::make<VirtualMachineCallback>(vm_state_);
                    std::cout << "Connected to VM: " << vm_name_ << std::endl;
                } else {
                    std::cout << "Failed to get VM: " << vm_name_ << std::endl;
                }
            } else {
                std::cout << "Invalid selection." << std::endl;
            }
        }
    }

    void operateVm() {
        while (true) {
            std::cout << "\n------Options------" << std::endl;
            std::cout << "1. Get VM Info" << std::endl;
            std::cout << "2. Start VM" << std::endl;
            std::cout << "3. Force Stop VM" << std::endl;
            std::cout << "4. Request VM Stop" << std::endl;
            std::cout << "5. Get VM State" << std::endl;
            std::cout << "6. Drop VM" << std::endl;
            std::cout << "7. Exit" << std::endl;
            std::cout << "-------------------" << std::endl;
            std::cout << "Choose an option: ";

            int option;
            std::cin >> option;
            std::cin.ignore();

            switch (option) {
                case 1: {
                    VmInfo vm_info;
                    if (vm_->getVmInfo(&vm_info).isOk()) {
                        std::cout << "VM Info - Name: " << vm_info.name
                                  << ", Early VM Enabled: " << (vm_info.early_vm ? "Yes" : "No")
                                  << ", Enabled: " << (vm_info.enabled ? "Yes" : "No")
                                  << ", Force Stop Supported: " << (vm_info.force_stop ? "Yes" : "No")
                                  << ", Number of vCPUs: " << vm_info.num_vcpus
                                  << ", CMA Size: " << vm_info.cma_size << " MB"
                                  << ", SWIOTLB Size: " << vm_info.swiotlb_size << " MB"
                                  << ", Total Memory Assigned: " << vm_info.total_memory << " MB"
                                  << ", VM ID: " << vm_info.vm_id
                                  << ", Mink UID: " << vm_info.mink_uid
                                  << std::endl;
                    } else {
                        std::cout << "Failed to get VM Info" << std::endl;
                    }
                    break;
                }
                case 2:
                    if (vm_) {
                        auto status = vm_->start(vm_callback_);
                        if (status.isOk()) {
                            std::cout << "Triggered VM: "<< vm_name_ <<" start"<<endl;
                        } else {
                          std::cout << "Failed to start VM: " << status.getDescription() << std::endl;
                        }
                    }
                    break;
                case 3:
                    if (vm_) {
                        if (*vm_state_ != VmState::Started && *vm_state_ != VmState::UserspaceReady) {
                            std::cout <<"Please register for callback first by calling start API."<<endl;
                        } else if (*vm_state_ == VmState::Stopped) {
                            std::cout<<"VM: " << vm_name_ <<" already stopped"<<endl;
                        } else {
                            auto status = vm_->stop(vm_callback_);
                            if (status.isOk()) {
                                std::cout << "Triggered VM: "<< vm_name_ <<" stop"<<endl;
                            } else {
                                std::cout << "Failed to stop VM " << status.getDescription() << std::endl;
                            }
                        }
                    }
                    break;
                case 4:
                    if (vm_) {
                        if (*vm_state_ != VmState::Started && *vm_state_ != VmState::UserspaceReady) {
                            std::cout <<"Please register for callback first by calling start API."<<endl;
                        } else if (*vm_state_ == VmState::Stopped) {
                            std::cout<<"VM: " << vm_name_ <<" already stopped"<<endl;
                        } else {
                            auto status = vm_->request_stop(vm_callback_);
                            if (status.isOk()) {
                                std::cout << "Requested VM: "<< vm_name_ <<" stop"<<endl;
                            } else {
                                std::cout << "Failed to request stop VM " << status.getDescription() << std::endl;
                            }
                        }
                    }
                    break;
                case 5:
                    if (*vm_state_ == VmState::NotStarted) {
                        std::cout <<"Please register for callback first by calling start API."<<endl;
                    } else {
                        std::cout << "Current VM state: " << to_string(*vm_state_) << std::endl;
                    }
                    break;
                case 6:
                    vm_ = nullptr;
                    vm_name_ = "";
                    vm_callback_ = nullptr;
                    std::cout << "Dropped VM connection." << std::endl;
                    return;
                case 7:
                    std::cout << "Exiting." << std::endl;
                    exit(0);
                default:
                    std::cout << "Invalid option. Try again." << std::endl;
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
        }
    }

    void run() {
        while (true) {
            if (vm_) {
                operateVm();
            } else {
                getVm();
            }
        }
    }

private:
    std::shared_ptr<IAvfQcvmManager> service_;
    std::shared_ptr<IVirtualMachine> vm_;
    std::string vm_name_;
    std::shared_ptr<std::atomic<VmState>> vm_state_;
    std::shared_ptr<IVirtualMachineCallback> vm_callback_;
};

int main() {
    ABinderProcess_startThreadPool();

    std::string service_name = std::string(IAvfQcvmManager::descriptor) + "/default";
    ndk::SpAIBinder sysBinder(AServiceManager_waitForService(service_name.c_str()));

    std::shared_ptr<IAvfQcvmManager> service = IAvfQcvmManager::fromBinder(sysBinder);
    if (!service) {
        std::cerr << "Failed to get virtualization service." << std::endl;
        return EXIT_FAILURE;
    }

    StateMachine machine(service);
    machine.run();

    ABinderProcess_joinThreadPool();
    return EXIT_FAILURE;
}
