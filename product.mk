ifeq ($(TARGET_USES_QMAA), true)
ifneq ($(TARGET_USES_QMAA_OVERRIDE_ANDROID_CORE),true)
PRODUCT_QC_AVF_ENABLE := false
else
PRODUCT_QC_AVF_ENABLE := true
endif
else
PRODUCT_QC_AVF_ENABLE := true
endif

ifeq ($(TARGET_BOARD_PLATFORM),bengal)
PRODUCT_QC_AVF_ENABLE := false
endif

ifeq ($(PRODUCT_QC_AVF_ENABLE),true)
PRODUCT_PACKAGES += early_vms_trusted_vm
PRODUCT_PACKAGES += early_vms_oem_vm
PRODUCT_PACKAGES += avf_qcvm_manager
PRODUCT_PACKAGES += qcvm_config.json
PRODUCT_PACKAGES += vendor.qti.qtvm.proxyclient-service
PRODUCT_PACKAGES_DEBUG += test_vm_client_rs
PRODUCT_PACKAGES_DEBUG += test_vm_client_cpp
PRODUCT_PACKAGES_DEBUG += avf_qcvm_test_client_rs
PRODUCT_PACKAGES_DEBUG += avf_qcvm_test_client_cpp

PRODUCT_PROPERTY_OVERRIDES += \
     ro.vendor.qtvm.auto.start=trustedvm
endif
