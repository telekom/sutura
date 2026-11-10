// Run only in hosted CI from a pinned go-ora v3 module checkout.
package main

import (
    "crypto/sha256"
    "crypto/sha512"
    "fmt"
    "hash"
    "github.com/sijms/go-ora/v3/network/security"
)

func emit(enc, integrity int, factory func() hash.Hash, folded bool) {
    key := make([]byte, 256)
    for index := range key { key[index] = byte(index) }
    iv := append([]byte(nil), key[32:64]...)
    if folded {
        for index := 0; index < 32; index++ {
            key[index] ^= 0xa5
            iv[index] ^= 0xa5
        }
    }
    masks, err := security.NewOracleNetworkHash2(factory(), key, iv, true)
    if err != nil { panic(err) }
    size := map[int]int{15:16,16:24,17:32}[enc]
    cryptor, err := security.NewOracleNetworkCBCEncrypter(key[:size], iv[:16])
    if err != nil { panic(err) }
    for index, message := range [][]byte{[]byte("first"), []byte("second"), {}} {
        input := append(append([]byte(nil), message...), masks.Compute(message)...)
        record, err := cryptor.Encrypt(input)
        if err != nil { panic(err) }
        fold := byte(0)
        if folded { fold = 1 }
        record = append(record, fold)
        fmt.Printf("%d %d %t %d %x\n", enc, integrity, folded, index, record)
    }
}

func main() {
    for _, enc := range []int{15, 16, 17} {
        for _, folded := range []bool{false, true} {
            emit(enc, 4, sha512.New, folded)
            emit(enc, 5, sha256.New, folded)
            emit(enc, 6, sha512.New384, folded)
        }
    }
}
