ENTRY(_start)
OUTPUT_ARCH(aarch64)
OUTPUT_FORMAT(elf64-littleaarch64)

KERNEL_LMA = 0x40000000;
KERNEL_VMA = 0xffff800040000000;

PHDRS
{
    boot PT_LOAD FLAGS(7);
    text PT_LOAD FLAGS(5);
    rodata PT_LOAD FLAGS(4);
    data PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = KERNEL_LMA;

    .boot : {
        *(.boot .boot.*)
    } :boot

    . = ALIGN(4096);
    __boot_end_lma = ABSOLUTE(.);
    __boot_size = __boot_end_lma - KERNEL_LMA;
    __text_lma = __boot_end_lma;
    __text_vma = KERNEL_VMA + __boot_size;

    . = __text_vma;
    __kernel_start = .;
    __executable_start = .;
    __etext = .;
    .text : AT(__text_lma) {
        *(.text._start)
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    __ex_table_start = .;
    __ex_table = .;
    .ex_table : AT(__text_lma + (. - __text_vma)) {
        *(.ex_table)
    } :rodata
    __ex_table_end = .;

    __eh_frame = .;
    .rodata : AT(__text_lma + (. - __text_vma)) {
        *(.rodata .rodata.*)
        *(.eh_frame_hdr)
    } :rodata
    .eh_frame : AT(__text_lma + (. - __text_vma)) {
        *(.eh_frame)
    } :rodata

    . = ALIGN(4096);
    __sinit_array = .;
    .init_array : AT(__text_lma + (. - __text_vma)) {
        *(.init_array .init_array.*)
    } :data
    __einit_array = .;

    .got : AT(__text_lma + (. - __text_vma)) {
        *(.got .got.*)
    } :data

    .data : AT(__text_lma + (. - __text_vma)) {
        *(.data .data.*)
    } :data

    . = ALIGN(4096);
    __cpu_local_start = .;
    .cpu_local : AT(__text_lma + (. - __text_vma)) {
        *(.cpu_local .cpu_local.*)
    } :data
    __cpu_local_end = .;

    __bss_start = .;
    .bss : AT(__text_lma + (. - __text_vma)) {
        *(.bss .bss.*)
        *(COMMON)
    } :data
    __bss_end = .;

    . = ALIGN(4096);
    __kernel_end = .;
    __kernel_end_lma = __text_lma + (__kernel_end - __text_vma);

    /DISCARD/ : {
        *(.comment)
        *(.note .note.*)
        *(.ARM.exidx .ARM.exidx.*)
    }
}
