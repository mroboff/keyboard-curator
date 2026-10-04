# Vendored ZMK headers

Unmodified copies of `app/include/dt-bindings/zmk/*.h` from
https://github.com/zmkfirmware/zmk at tag `v0.3.0`, under the MIT licence in
`LICENSE`.

`keys.h` and `hid_usage.h` are compiled into `kc-zmk` and parsed to build the
keycode catalogue. The rest are used by tests to check the behaviour catalogue
against the names ZMK actually defines.
