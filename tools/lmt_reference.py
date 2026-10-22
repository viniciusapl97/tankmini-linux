"""Reference LMT block for an AMP model file, by running calc_bias_gru_am4 /
calc_bias_gru from the official app's libamx_parser_lib.so (arm64) under
Unicorn. Dev tool for checking our own port; needs `pip install unicorn
pyelftools` and the library from captures/apk (not in git).

usage: lmt_reference.py LIB MODEL_FILE [am4|am3]
"""
import math, struct, sys
from elftools.elf.elffile import ELFFile
from elftools.elf.relocation import RelocationSection
from unicorn import Uc, UC_ARCH_ARM64, UC_MODE_ARM, UC_HOOK_CODE
from unicorn.arm64_const import *

lib, model = sys.argv[1], sys.argv[2]
kind = sys.argv[3] if len(sys.argv) > 3 else "am4"

mu = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
# Enable FP/SIMD (CPACR_EL1.FPEN).
mu.mem_map(0, 0x20000)
f = open(lib, "rb")
elf = ELFFile(f)
for seg in elf.iter_segments():
    if seg["p_type"] == "PT_LOAD":
        mu.mem_write(seg["p_vaddr"], seg.data())
syms = {s.name: s["st_value"] for s in elf.get_section_by_name(".dynsym").iter_symbols()}
STUB = 0x30000
mu.mem_map(STUB, 0x1000)
stubs = {}
for sec in elf.iter_sections():
    if not isinstance(sec, RelocationSection):
        continue
    symtab = elf.get_section(sec["sh_link"])
    for r in sec.iter_relocations():
        name = symtab.get_symbol(r["r_info_sym"]).name
        if syms.get(name):
            val = syms[name]
        else:
            val = STUB + 8 * len(stubs)
            stubs[val] = name
        mu.mem_write(r["r_offset"], struct.pack("<Q", val))

HEAP = 0x100000
mu.mem_map(HEAP, 0x100000)
heap_next = [HEAP]
STACK = 0x400000
mu.mem_map(STACK - 0x100000, 0x100000)
BUF = 0x80000
mu.mem_map(BUF, 0x10000)
data = open(model, "rb").read()
mu.mem_write(BUF, data)
END = 0x31000

def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]

def hook(uc, addr, size, _):
    name = stubs.get(addr)
    if addr == END:
        uc.emu_stop()
        return
    if name is None:
        return
    if name == "expf":
        x = struct.unpack("<f", struct.pack("<I", uc.reg_read(UC_ARM64_REG_S0) & 0xffffffff))[0]
        y = struct.unpack("<I", struct.pack("<f", f32(math.exp(x))))[0]
        uc.reg_write(UC_ARM64_REG_S0, y)
    elif name == "calloc":
        n = uc.reg_read(UC_ARM64_REG_X0) * uc.reg_read(UC_ARM64_REG_X1)
        p = heap_next[0]
        heap_next[0] += (n + 15) & ~15
        uc.mem_write(p, b"\0" * n)
        uc.reg_write(UC_ARM64_REG_X0, p)
    uc.reg_write(UC_ARM64_REG_PC, uc.reg_read(UC_ARM64_REG_LR))

mu.hook_add(UC_HOOK_CODE, hook, begin=STUB, end=STUB + 0x2000)
mu.reg_write(UC_ARM64_REG_CPACR_EL1, 0x300000)
mu.reg_write(UC_ARM64_REG_SP, STACK - 0x100)
mu.reg_write(UC_ARM64_REG_X0, BUF)
mu.reg_write(UC_ARM64_REG_LR, END)
entry = syms["calc_bias_gru_am4" if kind == "am4" else "calc_bias_gru"]
mu.emu_start(entry, END)
ret = mu.reg_read(UC_ARM64_REG_X0) & 0xffffffff
tail_at = 0x1ebc if kind == "am4" else 0x183c
out = bytes(mu.mem_read(BUF + tail_at, 68))
print("ret", hex(ret))
print(out.hex())
