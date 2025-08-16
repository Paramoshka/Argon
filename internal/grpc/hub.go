package grpc

import (
	"sync"

	"argon.github.io/ingress/internal/model"
)

type StreamHub struct {
	mu   sync.RWMutex
	next int64
	cons map[int64]chan model.Snapshot
	last model.Snapshot
}

func NewStreamHub() *StreamHub {
	return &StreamHub{cons: make(map[int64]chan model.Snapshot)}
}

func (h *StreamHub) Add() (int64, <-chan model.Snapshot, model.Snapshot) {
	h.mu.Lock()
	defer h.mu.Unlock()
	id := h.next
	h.next++
	ch := make(chan model.Snapshot, 1)
	h.cons[id] = ch
	return id, ch, h.last
}

func (h *StreamHub) Remove(id int64) {
	h.mu.Lock()
	defer h.mu.Unlock()
	delete(h.cons, id)
}

func (h *StreamHub) Broadcast(s model.Snapshot) {
	h.mu.Lock()
	h.last = s
	chs := make([]chan model.Snapshot, 0, len(h.cons))
	for _, c := range h.cons {
		chs = append(chs, c)
	}
	h.mu.Unlock()

	for _, c := range chs {
		select {
		case c <- s:
		default:
		}
	}
}
