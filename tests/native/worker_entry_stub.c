/* Synthetic loader contract only; never imports Python or processes a PDF. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#ifndef STUB_WITHOUT_ENTRY
__declspec(dllexport) int __cdecl Py_Main(int argc, wchar_t **argv) {
    (void)argv;
    DWORD count = 0;
    const char text[] = "SYNTHETIC_ENTRY_ARGS_OK";
    (void)WriteFile(GetStdHandle(STD_OUTPUT_HANDLE), text, (DWORD)(sizeof(text)-1), &count, NULL);
    return argc == 6 || argc == 3 ? 17 : 99;
}
#else
__declspec(dllexport) int __cdecl SyntheticOnly(void) { return 1; }
#endif
