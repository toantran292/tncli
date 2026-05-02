package config

import "testing"

func TestStartOrder_NoDeps(t *testing.T) {
	dir := &Dir{
		ServiceOrder: []string{"api", "worker"},
		Services: map[string]*Service{
			"api":    {Cmd: "go run ."},
			"worker": {Cmd: "go run ./worker"},
		},
	}
	g := BuildDepGraph(dir)
	order, err := g.StartOrder([]string{"api", "worker"})
	if err != nil {
		t.Fatal(err)
	}
	if len(order) != 2 {
		t.Fatalf("expected 2, got %d", len(order))
	}
}

func TestStartOrder_WithDeps(t *testing.T) {
	dir := &Dir{
		ServiceOrder: []string{"api", "worker", "scheduler"},
		Services: map[string]*Service{
			"worker":    {Cmd: "w"},
			"api":       {Cmd: "a", DependsOn: []string{"worker"}},
			"scheduler": {Cmd: "s", DependsOn: []string{"worker"}},
		},
	}
	g := BuildDepGraph(dir)
	order, err := g.StartOrder([]string{"api", "worker", "scheduler"})
	if err != nil {
		t.Fatal(err)
	}

	// worker must come before api and scheduler
	workerIdx := indexOf(order, "worker")
	apiIdx := indexOf(order, "api")
	schedIdx := indexOf(order, "scheduler")
	if workerIdx > apiIdx {
		t.Errorf("worker (%d) should come before api (%d)", workerIdx, apiIdx)
	}
	if workerIdx > schedIdx {
		t.Errorf("worker (%d) should come before scheduler (%d)", workerIdx, schedIdx)
	}
}

func TestStopOrder_Reverse(t *testing.T) {
	dir := &Dir{
		ServiceOrder: []string{"api", "worker"},
		Services: map[string]*Service{
			"worker": {Cmd: "w"},
			"api":    {Cmd: "a", DependsOn: []string{"worker"}},
		},
	}
	g := BuildDepGraph(dir)
	order, err := g.StopOrder([]string{"api", "worker"})
	if err != nil {
		t.Fatal(err)
	}

	// Stop: api first (depends on worker), then worker
	apiIdx := indexOf(order, "api")
	workerIdx := indexOf(order, "worker")
	if apiIdx > workerIdx {
		t.Errorf("api (%d) should stop before worker (%d)", apiIdx, workerIdx)
	}
}

func TestStartOrder_Cycle(t *testing.T) {
	dir := &Dir{
		ServiceOrder: []string{"a", "b"},
		Services: map[string]*Service{
			"a": {Cmd: "a", DependsOn: []string{"b"}},
			"b": {Cmd: "b", DependsOn: []string{"a"}},
		},
	}
	g := BuildDepGraph(dir)
	_, err := g.StartOrder([]string{"a", "b"})
	if err == nil {
		t.Fatal("expected cycle error")
	}
}

func TestStartOrder_TransitiveDeps(t *testing.T) {
	dir := &Dir{
		ServiceOrder: []string{"api", "worker", "db"},
		Services: map[string]*Service{
			"db":     {Cmd: "db"},
			"worker": {Cmd: "w", DependsOn: []string{"db"}},
			"api":    {Cmd: "a", DependsOn: []string{"worker"}},
		},
	}
	g := BuildDepGraph(dir)

	// Request only api — should pull in worker and db
	order, err := g.StartOrder([]string{"api"})
	if err != nil {
		t.Fatal(err)
	}
	if len(order) != 3 {
		t.Fatalf("expected 3 (transitive), got %d: %v", len(order), order)
	}
	dbIdx := indexOf(order, "db")
	workerIdx := indexOf(order, "worker")
	apiIdx := indexOf(order, "api")
	if dbIdx > workerIdx || workerIdx > apiIdx {
		t.Errorf("wrong order: %v", order)
	}
}

func TestStartOrder_Empty(t *testing.T) {
	dir := &Dir{
		ServiceOrder: []string{},
		Services:     map[string]*Service{},
	}
	g := BuildDepGraph(dir)
	order, err := g.StartOrder(nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(order) != 0 {
		t.Errorf("expected empty, got %v", order)
	}
}

func indexOf(ss []string, s string) int {
	for i, v := range ss {
		if v == s {
			return i
		}
	}
	return -1
}
