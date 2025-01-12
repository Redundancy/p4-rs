# P4
P4 a toy Rust wrapper for the P4 C++ API written in Rust and bridged using the CXX library.
Part of the intent is to play around with creating an abstraction around P4's lack of API, 
using structured types, using a language that can then be exposed to others.

This could have been done in C++... but what fun is that for learning things?

# Building
The version of the p4 OpenSSL dependency is determined (on windows) by: `strings librpc.lib | findstr /B OpenSSL`

This uses Conan to get the OpenSSL dependencies from a pre-built source.  
`conan install . -g deploy` from within the p4 folder should get OpenSSL and zlib.
This is *significantly* easier than building OpenSSL from scratch yourself.

# Current State

init / login is called, and prints errors when it doesn't work.
```
Hello, P4 Rust client!
client version=""
An error has occurred.
Error example 000001A21BCCEF80
	Severity 3 (error)
	Generic 38
	Count 3
		0: 824577061 (sub 37 sys 3 gen 38 args 1 sev 3 code 3109)
		0: %errortext%
		1: 824577036 (sub 12 sys 3 gen 38 args 1 sev 3 code 3084)
		1: TCP connect to %host% failed.
		2: 807804929 (sub 1 sys 8 gen 38 args 0 sev 3 code 8193)
		2: Connect to server failed; check $P4PORT.
		errortext = No such host is known. 
		host = perforce:1666
```