# measure
like `time(1)`, but for windows and likely simpler.

## install
```
cargo install --git https://github.com/museun/measure -f
```
or from source
```
git clone https://github.com/museun/measure.git
cd measure
cargo install --path . -f
```

## usage
```
measure your_command --and its args
```
```
peak    169.38MiB       # the peak memory usages
real    6.670s          # the 'real' time it took
user    0.000s          # how much time was spent in the userland
sys     0.031s          # how much time was spent in the kernel
```
