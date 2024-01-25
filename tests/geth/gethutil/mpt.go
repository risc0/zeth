package gethutil

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/trie"
)

type Transaction struct {
	Essence struct {
		Eip1559 *EIP1559Transaction `json:"Eip1559"`
		Legacy  *LegacyTransaction  `json:"Legacy"`
	} `json:"essence"`
	Signature struct {
		R *hexutil.Big `json:"r"`
		S *hexutil.Big `json:"s"`
		V int64        `json:"v"`
	} `json:"signature"`
}

type EIP1559Transaction struct {
	AccessList           []common.Address `json:"access_list"`
	ChainId              int64            `json:"chain_id"`
	Data                 hexutil.Bytes    `json:"data"`
	GasLimit             hexutil.Uint64   `json:"gas_limit"`
	MaxFeePerGas         *hexutil.Big     `json:"max_fee_per_gas"`
	MaxPriorityFeePerGas *hexutil.Big     `json:"max_priority_fee_per_gas"`
	To                   To               `json:"to"`
	Nonce                uint64           `json:"nonce"`
	Value                *hexutil.Big     `json:"value"`
}

type LegacyTransaction struct {
	ChainId  int64          `json:"chain_id"`
	Data     hexutil.Bytes  `json:"data"`
	GasLimit hexutil.Uint64 `json:"gas_limit"`
	GasPrice *hexutil.Big   `json:"gas_price"`
	Nonce    uint64         `json:"nonce"`
	To       To             `json:"to"`
	Value    *hexutil.Big   `json:"value"`
}

type To struct {
	Create bool            `json:"create"`
	Call   *common.Address `json:"Call"`
}

func (t *To) UnmarshalJSON(data []byte) error {
	type to struct {
		Call *common.Address `json:"Call"`
	}
	if string(data) == `"Create"` {
		t.Create = true
		return nil
	} else {
		to := &to{}
		if err := json.Unmarshal(data, to); err != nil {
			return err
		}
		t.Call = to.Call
		return nil
	}
}

type Result struct {
	Root common.Hash `json:"root"`
	Rlps []string    `json:"rlps"`
}

func MptRoot(txs []Transaction) *Result {
	var txs2 []*types.Transaction
	result := &Result{}
	for _, tx := range txs {
		var tx2 *types.Transaction
		if tx.Essence.Eip1559 != nil {
			tx2 = types.NewTx(&types.DynamicFeeTx{
				ChainID:    big.NewInt(tx.Essence.Eip1559.ChainId),
				Nonce:      tx.Essence.Eip1559.Nonce,
				GasTipCap:  tx.Essence.Eip1559.MaxPriorityFeePerGas.ToInt(),
				GasFeeCap:  tx.Essence.Eip1559.MaxFeePerGas.ToInt(),
				Gas:        uint64(tx.Essence.Eip1559.GasLimit),
				Value:      tx.Essence.Eip1559.Value.ToInt(),
				Data:       tx.Essence.Eip1559.Data,
				To:         tx.Essence.Eip1559.To.Call,
				AccessList: []types.AccessTuple{},
				V:          big.NewInt(tx.Signature.V),
				R:          tx.Signature.R.ToInt(),
				S:          tx.Signature.S.ToInt(),
			})
		} else {
			tx2 = types.NewTx(&types.LegacyTx{
				Nonce:    tx.Essence.Legacy.Nonce,
				GasPrice: tx.Essence.Legacy.GasPrice.ToInt(),
				Gas:      uint64(tx.Essence.Legacy.GasLimit),
				Value:    tx.Essence.Legacy.Value.ToInt(),
				Data:     tx.Essence.Legacy.Data,
				To:       tx.Essence.Legacy.To.Call,
				V:        big.NewInt(tx.Signature.V),
				R:        tx.Signature.R.ToInt(),
				S:        tx.Signature.S.ToInt(),
			})
		}

		var buf bytes.Buffer
		tx2.EncodeRLP(&buf)
		result.Rlps = append(result.Rlps, hex.EncodeToString(buf.Bytes()))
		txs2 = append(txs2, tx2)
	}
	root := types.DeriveSha(types.Transactions(txs2), trie.NewStackTrie(nil))
	result.Root = root
	return result
}
