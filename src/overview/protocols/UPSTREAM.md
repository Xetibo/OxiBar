# Native toplevel protocols

Wire interfaces adapted from `hyprwm/hyprland-protocols` revision
`4c27ba9ce104782bf718aabf59df3f503afe7caa`. Copyright and BSD-3-Clause
terms are preserved in each XML file. Documentation is shortened; request/event
order, versions, argument types and enum values match upstream.

Export uses version 2's exact foreign-toplevel handle, never its legacy 32-bit
window address. Mapping combines both address words without truncation.
