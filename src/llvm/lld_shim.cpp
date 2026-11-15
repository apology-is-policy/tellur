// In-process LLD for the embedded LLVM backend. The driver is chosen from argv[0]:
// "ld64.lld" (Mach-O), "ld.lld" (ELF) or "lld-link" (COFF).
#include "lld/Common/Driver.h"
#include "llvm/Support/raw_ostream.h"

#include "llvm/Support/SHA256.h"

#include <algorithm>
#include <vector>

LLD_HAS_DRIVER(macho)
LLD_HAS_DRIVER(elf)
LLD_HAS_DRIVER(coff)

extern "C" int tellur_lld_link(int argc, const char **argv, int *can_run_again) {
    std::vector<const char *> args(argv, argv + argc);
    lld::Result result = lld::lldMain(args, llvm::outs(), llvm::errs(),
                                      {{lld::Darwin, &lld::macho::link},
                                       {lld::Gnu, &lld::elf::link},
                                       {lld::WinLink, &lld::coff::link}});
    llvm::outs().flush();
    llvm::errs().flush();
    *can_run_again = result.canRunAgain ? 1 : 0;
    return result.retCode;
}

extern "C" void tellur_sha256(const uint8_t *data, size_t length, uint8_t *out) {
    auto digest = llvm::SHA256::hash({data, length});
    std::copy(digest.begin(), digest.end(), out);
}
