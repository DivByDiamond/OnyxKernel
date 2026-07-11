OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {
    . = 0x10000;
    .text : ALIGN(4) { KEEP(*(.text.start)) *(.text .text.*) }
    .rodata : ALIGN(8) { *(.rodata .srodata .rodata.* .srodata.*) }
    .data : ALIGN(8) { *(.data .sdata .data.* .sdata.*) }
    /* Align .bss to a page boundary so it doesn't share a page with
       .rodata or .data. The ONX loader maps each segment's pages
       independently; if .bss shares a page with a read-only segment,
       that page stays read-only and writes to .bss variables page-fault. */
    . = ALIGN(4096); __bss_start = .;
    .bss : ALIGN(4096) { *(.bss .sbss .bss.* .sbss.* COMMON) }
    . = ALIGN(8); __bss_end = .;
    /DISCARD/ : { *(.note .note.*) *(.comment) *(.eh_frame .eh_frame.*) *(.riscv.attributes) }
}
