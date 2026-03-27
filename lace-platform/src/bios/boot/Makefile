all: stage1.bin stage2.bin

stage1.bin: stage1.s boot_defs.s boot_utils.s
	as stage1.s -o stage1.o
	ld -Ttext 0x7c00 --oformat binary -o stage1.bin stage1.o

stage2.bin: stage2.s boot_defs.s boot_utils.s
	as stage2.s -o stage2.o
	ld -Ttext 0x8000 --oformat binary -o stage2.bin stage2.o

clean:
	rm -f *.o *.bin
