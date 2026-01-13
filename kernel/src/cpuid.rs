use alloc::vec::Vec;
use raw_cpuid::{CpuId, CpuIdReaderNative};

pub struct CpuidState {
    features: CpuFeatures,
}

impl CpuidState {
    pub fn new() -> Self {
        let features = CpuFeatures::new();

        Self { features }
    }

    pub fn features(&self) -> &Vec<(&'static str, bool)> {
        self.features.features()
    }

    pub fn extended_features(&self) -> &Vec<(&'static str, bool)> {
        self.features.extended_features()
    }

    pub fn extended_state_features(&self) -> &ExtendedStateFeatures {
        self.features.extended_state_features()
    }

    pub fn vendor_info(&self) -> &VendorInfo {
        self.features.vendor_info()
    }

    pub fn has_xsave(&self) -> bool {
        self.features.has_xsave()
    }

    pub fn leaf_0xd_0(&self) -> [u32; 4] {
        self.features.leaf(0xD, 0)
    }

    pub fn leaf_0xd_1(&self) -> [u32; 4] {
        self.features.leaf(0xD, 1)
    }

    pub fn leaf_0x1_0(&self) -> [u32; 4] {
        self.features.leaf(0x1, 0)
    }

    pub fn has_avx2(&self) -> bool {
        self.features.has_avx2()
    }

    pub fn leaf(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        self.features.leaf(leaf, subleaf)
    }

    pub fn cpu_features(&self) -> &CpuFeatures {
        &self.features
    }
}

pub struct ExtendedStateFeatures {
    supports: Vec<(&'static str, bool)>,
    sizes: Vec<(&'static str, u32)>,
}

impl ExtendedStateFeatures {
    pub fn supports(&self) -> &Vec<(&'static str, bool)> {
        &self.supports
    }

    pub fn sizes(&self) -> &Vec<(&'static str, u32)> {
        &self.sizes
    }
}

pub struct CpuFeatures {
    vendor_info: VendorInfo,
    features: Vec<(&'static str, bool)>,
    extended_features: Vec<(&'static str, bool)>,
    extended_state_features: ExtendedStateFeatures,
    cpuid: CpuId<CpuIdReaderNative>,
}

impl CpuFeatures {
    pub fn new() -> Self {
        let cpuid = CpuId::new();
        let features = build_features(&cpuid);
        let extended_features = build_extended_features(&cpuid);
        let extended_state_features = build_extended_state_features(&cpuid);
        let vendor_info = build_vendor_info(&cpuid);
        CpuFeatures {
            cpuid,
            vendor_info,
            features,
            extended_features,
            extended_state_features,
        }
    }

    pub fn vendor_info(&self) -> &VendorInfo {
        &self.vendor_info
    }

    pub fn features(&self) -> &Vec<(&'static str, bool)> {
        &self.features
    }

    pub fn extended_features(&self) -> &Vec<(&'static str, bool)> {
        &self.extended_features
    }

    pub fn extended_state_features(&self) -> &ExtendedStateFeatures {
        &self.extended_state_features
    }

    pub fn has_xsave(&self) -> bool {
        let cpuid = CpuId::new();
        let fi = cpuid.get_feature_info().unwrap();
        fi.has_xsave()
    }

    pub fn has_avx2(&self) -> bool {
        let efi = self.cpuid.get_extended_feature_info().unwrap();
        efi.has_avx2()
    }

    pub fn leaf(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        // Directly query leaf 0xD subleaf 0
        unsafe {
            let result = core::arch::x86_64::__cpuid_count(leaf, subleaf);
            [result.eax, result.ebx, result.ecx, result.edx]
        }
    }

    // MSR-related feature checks
    pub fn has_mtrr(&self) -> bool {
        self.cpuid
            .get_feature_info()
            .is_some_and(|fi| fi.has_mtrr())
    }

    pub fn has_pat(&self) -> bool {
        self.cpuid
            .get_feature_info()
            .is_some_and(|fi| fi.has_pat())
    }

    pub fn has_mce(&self) -> bool {
        self.cpuid
            .get_feature_info()
            .is_some_and(|fi| fi.has_mce())
    }

    pub fn has_mca(&self) -> bool {
        self.cpuid
            .get_feature_info()
            .is_some_and(|fi| fi.has_mca())
    }

    pub fn has_rdtscp(&self) -> bool {
        self.cpuid
            .get_extended_processor_and_feature_identifiers()
            .is_some_and(|efi| efi.has_rdtscp())
    }

    pub fn has_tsc_adjust(&self) -> bool {
        self.cpuid
            .get_extended_feature_info()
            .is_some_and(|efi| efi.has_tsc_adjust_msr())
    }
}

/// Returns TSC frequency in Hz from CPUID leaf 0x15, or processor base
/// frequency from leaf 0x16 as fallback.
pub fn tsc_frequency() -> Option<u64> {
    let cpuid = CpuId::new();

    // Try leaf 0x15 first (TSC/Crystal Clock info)
    if let Some(tsc_info) = cpuid.get_tsc_info()
        && let Some(freq) = tsc_info.tsc_frequency() {
            return Some(freq);
        }

    // Fall back to leaf 0x16 (Processor Frequency Info)
    let freq_info = cpuid.get_processor_frequency_info()?;
    let base_mhz = freq_info.processor_base_frequency();
    if base_mhz > 0 {
        return Some(base_mhz as u64 * 1_000_000);
    }

    None
}

pub struct VendorInfo {
    pub intel: bool,
    pub amd: bool,
}

fn build_vendor_info(cpuid: &CpuId<CpuIdReaderNative>) -> VendorInfo {
    match cpuid.get_vendor_info() {
        Some(vendor) => {
            let vendor_str = vendor.as_str();
            VendorInfo {
                intel: vendor_str == "GenuineIntel",
                amd: vendor_str == "AuthenticAMD",
            }
        }
        None => VendorInfo {
            intel: false,
            amd: false,
        },
    }
}

fn build_extended_state_features(cpuid: &CpuId<CpuIdReaderNative>) -> ExtendedStateFeatures {
    let esfi = cpuid.get_extended_state_info().unwrap();

    let mut supports: Vec<(&str, bool)> = Vec::new();
    macro_rules! push_supports {
        ($m:ident) => {
            let name = stringify!($m);
            supports.push((name, esfi.$m()));
        };
    }

    let mut sizes: Vec<(&str, u32)> = Vec::new();
    macro_rules! push_size {
        ($m:ident) => {
            let name = stringify!($m);
            sizes.push((name, esfi.$m()));
        };
    }

    push_supports!(xcr0_supports_legacy_x87);
    push_supports!(xcr0_supports_sse_128);
    push_supports!(xcr0_supports_avx_256);
    push_supports!(xcr0_supports_mpx_bndregs);
    push_supports!(xcr0_supports_mpx_bndcsr);
    push_supports!(xcr0_supports_avx512_opmask);
    push_supports!(xcr0_supports_avx512_zmm_hi256);
    push_supports!(xcr0_supports_avx512_zmm_hi16);
    push_supports!(xcr0_supports_pkru);
    push_supports!(ia32_xss_supports_pt);
    push_supports!(ia32_xss_supports_hdc);
    push_supports!(has_xsaveopt);
    push_supports!(has_xsavec);
    push_supports!(has_xsaves_xrstors);

    push_size!(xsave_area_size_enabled_features);
    push_size!(xsave_area_size_supported_features);
    push_size!(xsave_size);

    ExtendedStateFeatures { supports, sizes }
}

fn build_extended_features(cpuid: &CpuId<CpuIdReaderNative>) -> Vec<(&'static str, bool)> {
    let efi = cpuid.get_extended_feature_info().unwrap();
    let mut out: Vec<(&str, bool)> = Vec::new();

    macro_rules! push_has {
        ($m:ident) => {
            let name = stringify!($m).strip_prefix("has_").unwrap();
            out.push((name, efi.$m()));
        };
    }

    push_has!(has_adx);
    push_has!(has_amx_bf16);
    push_has!(has_amx_int8);
    push_has!(has_amx_tile);
    push_has!(has_avx2);
    push_has!(has_avx10);
    push_has!(has_avx512_4fmaps);
    push_has!(has_avx512_4vnniw);
    push_has!(has_avx512_bf16);
    push_has!(has_avx512_fp16);
    push_has!(has_avx512_ifma);
    push_has!(has_avx512_vp2intersect);
    push_has!(has_avx512bitalg);
    push_has!(has_avx512bw);
    push_has!(has_avx512cd);
    push_has!(has_avx512dq);
    push_has!(has_avx512er);
    push_has!(has_avx512f);
    push_has!(has_avx512pf);
    push_has!(has_avx512vbmi);
    push_has!(has_avx512vbmi2);
    push_has!(has_avx512vl);
    push_has!(has_avx512vnni);
    push_has!(has_avx512vpopcntdq);
    push_has!(has_avx_ifma);
    push_has!(has_avx_ne_convert);
    push_has!(has_avx_vnni);
    push_has!(has_avx_vnni_int8);
    push_has!(has_avx_vnni_int16);
    push_has!(has_bmi1);
    push_has!(has_bmi2);
    push_has!(has_cet_ss);
    push_has!(has_cet_sss);
    push_has!(has_clflushopt);
    push_has!(has_clwb);
    push_has!(has_fdp);
    push_has!(has_fpu_cs_ds_deprecated);
    push_has!(has_fsgsbase);
    push_has!(has_fsrcrs);
    push_has!(has_fsrs);
    push_has!(has_fzrm);
    push_has!(has_gfni);
    push_has!(has_hle);
    push_has!(has_hreset);
    push_has!(has_invd_disable_post_bios_done);
    push_has!(has_invpcid);
    push_has!(has_la57);
    push_has!(has_lam);
    push_has!(has_mpx);
    push_has!(has_msrlist);
    push_has!(has_ospke);
    push_has!(has_pku);
    push_has!(has_prefetchi);
    push_has!(has_prefetchwt1);
    push_has!(has_processor_trace);
    push_has!(has_rdpid);
    push_has!(has_rdseed);
    push_has!(has_rdta);
    push_has!(has_rdtm);
    push_has!(has_rep_movsb_stosb);
    push_has!(has_rtm);
    push_has!(has_sgx);
    push_has!(has_sgx_lc);
    push_has!(has_sha);
    push_has!(has_smap);
    push_has!(has_smep);
    push_has!(has_tme_en);
    push_has!(has_tsc_adjust_msr);
    push_has!(has_uiret_uif);
    push_has!(has_umip);
    push_has!(has_vaes);
    push_has!(has_vpclmulqdq);
    push_has!(has_waitpkg);

    out
}

fn build_features(cpuid: &CpuId<CpuIdReaderNative>) -> Vec<(&'static str, bool)> {
    let fi = cpuid.get_feature_info().unwrap();

    let mut out: Vec<(&str, bool)> = Vec::new();

    macro_rules! push_has {
        ($m:ident) => {
            let name = stringify!($m).strip_prefix("has_").unwrap();
            out.push((name, fi.$m()));
        };
    }

    push_has!(has_acpi);
    push_has!(has_aesni);
    push_has!(has_apic);
    push_has!(has_avx);
    push_has!(has_clflush);
    push_has!(has_cmov);
    push_has!(has_cmpxchg8b);
    push_has!(has_cmpxchg16b);
    push_has!(has_cnxtid);
    push_has!(has_cpl);
    push_has!(has_dca);
    push_has!(has_de);
    push_has!(has_ds);
    push_has!(has_ds_area);
    push_has!(has_eist);
    push_has!(has_f16c);
    push_has!(has_fma);
    push_has!(has_fpu);
    push_has!(has_fxsave_fxstor);
    push_has!(has_htt);
    push_has!(has_hypervisor);
    push_has!(has_mca);
    push_has!(has_mce);
    push_has!(has_mmx);
    push_has!(has_monitor_mwait);
    push_has!(has_movbe);
    push_has!(has_msr);
    push_has!(has_mtrr);
    push_has!(has_oxsave);
    push_has!(has_pae);
    push_has!(has_pat);
    push_has!(has_pbe);
    push_has!(has_pcid);
    push_has!(has_pclmulqdq);
    push_has!(has_pdcm);
    push_has!(has_pge);
    push_has!(has_popcnt);
    push_has!(has_pse);
    push_has!(has_pse36);
    push_has!(has_psn);
    push_has!(has_rdrand);
    push_has!(has_smx);
    push_has!(has_ss);
    push_has!(has_sse);
    push_has!(has_sse2);
    push_has!(has_sse3);
    push_has!(has_sse41);
    push_has!(has_sse42);
    push_has!(has_ssse3);
    push_has!(has_sysenter_sysexit);
    push_has!(has_tm);
    push_has!(has_tm2);
    push_has!(has_tsc);
    push_has!(has_tsc_deadline);
    push_has!(has_vme);
    push_has!(has_vmx);
    push_has!(has_x2apic);
    push_has!(has_xsave);

    out
}
