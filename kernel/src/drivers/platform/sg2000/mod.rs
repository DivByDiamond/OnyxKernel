//! SG2000 (Milk-V Duo S) board bring-up: CRU clock gating and FMUX pinmux.
//!
//! All base addresses and register offsets below are taken directly from
//! `SG2000_TRM_V1.0-alpha.pdf` (chapters 6.9/6.10 "CLK_DIV CRG" and 8.1
//! "管脚复用 PINMUX", `milkv-duo/duo-files` on GitHub, `duo-s/datasheet/`),
//! not guessed — see the doc comments on each constant/function for the
//! exact chapter each value comes from.
pub mod clk;
pub mod pinmux;

pub use clk::{enable_dsi, enable_spi};
pub use pinmux::spi1_pins;

/// CLK_DIV CRG base (TRM §6.9 "Clock Gen base address").
pub const CRU_BASE: usize = 0x0300_2000;
/// PINMUX/FMUX controller base (TRM memory map, `0x0300_1000..0x0300_1FFF`).
pub const PINMUX_BASE: usize = 0x0300_1000;

/// SPI0..SPI3 controller MMIO bases (TRM 表格 18‑6 "芯片的 4 组 SPI 模块基地址").
pub const SPI_BASES: [usize; 4] = [0x0418_0000, 0x0419_0000, 0x041A_0000, 0x041B_0000];
/// DW-style DSI MAC register block base (TRM memory map: `dsi_mac 控制寄存器`).
pub const DSI_MAC_BASE: usize = 0x0A08_A000;
/// MIPI Tx D-PHY register block base (TRM §16.5.6 "MIPI Tx PHY 寄存器位置").
pub const DSI_PHY_BASE: usize = 0x0A0D_1000;
