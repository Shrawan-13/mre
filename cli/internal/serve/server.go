package serve

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"fmt"
	"io"
	"log"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/Shrawan-13/mre/cli/internal/project"
	"github.com/Shrawan-13/mre/cli/internal/version"
)

const pollTimeout = 25 * time.Second

// LogLine is one event reported by the Android client.
type LogLine struct {
	Index int
	Time  time.Time
	Text  string
}

// Server owns one immutable bundle revision at a time.
type Server struct {
	root      string
	cfg       project.Config
	token     string
	sessionID string

	mu        sync.RWMutex
	bundle    Bundle
	revision  uint64
	waiters   map[chan uint64]struct{}
	logs      []LogLine
	logWaiter map[chan int]struct{}
}

// NewToken creates the pairing secret embedded in the QR.
func NewToken() (string, error) {
	buf := make([]byte, 16)
	if _, err := rand.Read(buf); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf), nil
}

// New builds a server with revision 1 of the project.
func New(root string, cfg project.Config, token string) (*Server, error) {
	bundle, err := Build(root, cfg, 1)
	if err != nil {
		return nil, err
	}
	buf := make([]byte, 8)
	if _, err := rand.Read(buf); err != nil {
		return nil, err
	}
	return &Server{
		root:      root,
		cfg:       cfg,
		token:     token,
		sessionID: hex.EncodeToString(buf),
		bundle:    bundle,
		revision:  1,
		waiters:   map[chan uint64]struct{}{},
		logWaiter: map[chan int]struct{}{},
	}, nil
}

// Token returns the pairing secret for the QR and `mre log`.
func (s *Server) Token() string { return s.token }

// Session returns the session id clients must echo on every request.
func (s *Server) Session() string { return s.sessionID }

// Revision returns the current bundle revision.
func (s *Server) Revision() uint64 {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.revision
}

// Update publishes a new bundle and wakes every long-poll waiter.
func (s *Server) Update(bundle Bundle) uint64 {
	s.mu.Lock()
	s.revision++
	bundle.Revision = s.revision
	s.bundle = bundle
	revision := s.revision
	for ch := range s.waiters {
		select {
		case ch <- revision:
		default:
		}
	}
	s.mu.Unlock()
	return revision
}

// AddLog records a client event and wakes log followers.
func (s *Server) AddLog(text string) {
	s.mu.Lock()
	line := LogLine{Index: len(s.logs) + 1, Time: time.Now(), Text: strings.TrimSpace(text)}
	s.logs = append(s.logs, line)
	for ch := range s.logWaiter {
		select {
		case ch <- line.Index:
		default:
		}
	}
	s.mu.Unlock()
}

// Handler returns the HTTP handler for the dev server.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/manifest", s.manifest)
	mux.HandleFunc("GET /api/wait", s.wait)
	mux.HandleFunc("GET /api/bundle", s.bundleHandler)
	mux.HandleFunc("POST /api/events", s.events)
	mux.HandleFunc("GET /api/logs", s.logsHandler)
	mux.HandleFunc("GET /", s.status)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/") && !s.authorized(r) {
			w.Header().Set("WWW-Authenticate", "MreToken")
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}
		mux.ServeHTTP(w, r)
	})
}

func (s *Server) authorized(r *http.Request) bool {
	provided := r.Header.Get("X-Mre-Token")
	if provided == "" {
		provided = r.URL.Query().Get("token")
	}
	return subtle.ConstantTimeCompare([]byte(provided), []byte(s.token)) == 1
}

func (s *Server) manifest(w http.ResponseWriter, _ *http.Request) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	fmt.Fprintf(w, "protocol %s\nrevision %d\nhash %s\nmodules %d\nsession %s\nproject %s\n",
		version.Protocol, s.revision, s.bundle.Hash, len(s.bundle.Modules), s.sessionID, s.cfg.Name)
}

func (s *Server) wait(w http.ResponseWriter, r *http.Request) {
	since, err := strconv.ParseUint(r.URL.Query().Get("since"), 10, 64)
	if err != nil {
		since = 0
	}
	s.mu.RLock()
	current := s.revision
	s.mu.RUnlock()
	if current > since {
		writeRevision(w, current)
		return
	}

	ch := make(chan uint64, 1)
	s.mu.Lock()
	s.waiters[ch] = struct{}{}
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		delete(s.waiters, ch)
		s.mu.Unlock()
	}()

	timer := time.NewTimer(pollTimeout)
	defer timer.Stop()
	select {
	case revision := <-ch:
		writeRevision(w, revision)
	case <-timer.C:
		w.WriteHeader(http.StatusNotModified)
	case <-r.Context().Done():
	}
}

func writeRevision(w http.ResponseWriter, revision uint64) {
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	fmt.Fprintf(w, "revision %d\n", revision)
	if err := http.NewResponseController(w).Flush(); err != nil {
		return
	}
}

func (s *Server) bundleHandler(w http.ResponseWriter, _ *http.Request) {
	s.mu.RLock()
	bundle := s.bundle
	s.mu.RUnlock()
	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("Cache-Control", "no-store")
	if err := Encode(w, bundle); err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
	}
}

func (s *Server) events(w http.ResponseWriter, r *http.Request) {
	body, err := io.ReadAll(io.LimitReader(r.Body, 8192))
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	text := strings.TrimSpace(string(body))
	if text == "" {
		w.WriteHeader(http.StatusNoContent)
		return
	}
	s.AddLog(text)
	log.Printf("device: %s", text)
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) logsHandler(w http.ResponseWriter, r *http.Request) {
	since, _ := strconv.Atoi(r.URL.Query().Get("since"))
	s.mu.RLock()
	pending := len(s.logs) - since
	if pending > 0 {
		for _, line := range s.logs[since:] {
			fmt.Fprintf(w, "%d %s %s\n", line.Index, line.Time.Format(time.RFC3339), line.Text)
		}
		s.mu.RUnlock()
		return
	}
	s.mu.RUnlock()

	ch := make(chan int, 1)
	s.mu.Lock()
	s.logWaiter[ch] = struct{}{}
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		delete(s.logWaiter, ch)
		s.mu.Unlock()
	}()

	timer := time.NewTimer(pollTimeout)
	defer timer.Stop()
	select {
	case <-ch:
		s.mu.RLock()
		for _, line := range s.logs[since:] {
			fmt.Fprintf(w, "%d %s %s\n", line.Index, line.Time.Format(time.RFC3339), line.Text)
		}
		s.mu.RUnlock()
	case <-timer.C:
		w.WriteHeader(http.StatusNoContent)
	case <-r.Context().Done():
	}
}

func (s *Server) status(w http.ResponseWriter, _ *http.Request) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	fmt.Fprintf(w, `<!doctype html><meta charset="utf-8"><title>MRE dev</title>
<style>body{background:#0B1120;color:#E2E8F0;font:14px system-ui;padding:32px}
code{color:#38BDF8}h1{margin-top:0}</style>
<h1>MRE dev server</h1><p>Project <code>%s</code></p>
<p>Protocol <code>%s</code> · revision <code>%d</code> · modules <code>%d</code></p>
<p>Hash <code>%s</code></p><p>Scan the QR from the terminal to connect.</p>`,
		s.cfg.Name, version.Protocol, s.revision, len(s.bundle.Modules), s.bundle.Hash)
}

// Serve runs until the context is cancelled.
func (s *Server) Serve(ctx context.Context, addr string) error {
	server := &http.Server{
		Addr:              addr,
		Handler:           s.Handler(),
		ReadHeaderTimeout: 5 * time.Second,
		IdleTimeout:       60 * time.Second,
	}
	done := make(chan error, 1)
	go func() { done <- server.ListenAndServe() }()
	select {
	case <-ctx.Done():
		shutdown, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		_ = server.Shutdown(shutdown)
		return nil
	case err := <-done:
		if err == http.ErrServerClosed {
			return nil
		}
		return err
	}
}
