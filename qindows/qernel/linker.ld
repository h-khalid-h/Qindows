/* ═══════════════════════════════════════════
   QINDOWS QERNEL — Linker Script
   Defines the memory layout for the bare-metal kernel.
   ═══════════════════════════════════════════ */

ENTRY(_start)

SECTIONS {
    /* Kernel loads at 2 MiB (above bootloader/firmware) */
    . = 2M;

    /* Code section — executable, read-only */
    .text : ALIGN(4K) {
        *(.text .text.*)
    }

    /* Read-only data (constants, string literals) */
    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    }

    /* Initialized data */
    .data : ALIGN(4K) {
        *(.data .data.*)
    }

    /* Uninitialized data (zeroed at boot) */
    .bss : ALIGN(4K) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* Stack — grows downward from 64 KiB mark */
    . = ALIGN(4K);
    __stack_bottom = .;
    . += 64K;
    __stack_top = .;

    /* Discard unnecessary sections */
    /DISCARD/ : {
        *(.eh_frame)
        *(.note*)
        *(.comment*)
    }
}
