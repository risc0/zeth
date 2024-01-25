package main

/*
   #include <stdlib.h>
*/
import "C"
import (
	"encoding/json"
	"fmt"
	"main/gethutil"
	"unsafe"
)

//export MptRoot
func MptRoot(configStr *C.char) *C.char {
	var txs []gethutil.Transaction
	err := json.Unmarshal([]byte(C.GoString(configStr)), &txs)
	if err != nil {
		return C.CString(fmt.Sprintf("Failed to unmarshal txs, err: %v", err))
	}

	executionResults := gethutil.MptRoot(txs)
	bytes, err := json.MarshalIndent(executionResults, "", "  ")
	if err != nil {
		return C.CString(fmt.Sprintf("Failed to marshal []ExecutionResult, err: %v", err))
	}

	return C.CString(string(bytes))
}

//export FreeString
func FreeString(str *C.char) {
	C.free(unsafe.Pointer(str))
}

func main() {}
