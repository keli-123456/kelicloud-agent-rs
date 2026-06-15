# Tunnel Forwarding Phase 2 Control Plane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a live tunnel control plane so `kelicloud-agent-rs` can register tunnel capability, receive rule revisions, and report rule status through backend `/api/clients/tunnel`.

**Architecture:** Phase 2 adds a separate JSON WebSocket control channel that is independent from the existing report WebSocket and does not carry tunnel data. The backend stores per-client tunnel control capability/connection state and computes rule-sync payloads from Phase 1 `TunnelRule` records; the Rust agent connects, sends `hello`, receives `rule_sync`, and reports `rule_ack`/heartbeat without opening listeners.

**Tech Stack:** Go + Gin + Gorilla WebSocket + GORM in `kelicloud`; Rust + tungstenite + serde in `kelicloud-agent-rs`; existing token auth, user feature gates, and test harnesses.

---

## Scope Check

This plan implements only the Phase 2 control plane:

- Agent capability registration.
- Backend `/api/clients/tunnel` control WebSocket.
- Rule selection for authenticated client group membership.
- Deterministic rule revision fingerprint.
- Heartbeat, `rule_ack`, and basic per-rule status updates.
- Backend status derivation that distinguishes unsupported, disconnected, and ready tunnel-control state.

This plan does not:

- Implement KTP binary frames.
- Open TCP listeners on agents.
- Dial target services from agents.
- Relay or proxy RDP/TCP bytes.
- Add TLS tunnel data sessions.
- Change existing `/api/clients/report` online presence semantics.

## File Structure

Backend repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud`

- Modify `database/models/tunnel.go`
  - Adds `ClientTunnelState` and constants for tunnel control protocol/capabilities.
- Modify `database/dbcore/dbcore.go`
  - Adds `&models.ClientTunnelState{}` to auto migration.
- Create `database/tunnel/control.go`
  - Owns capability state upsert/disconnect, rule selection for one client, revision hashing, and control-aware group readiness.
- Create `database/tunnel/control_test.go`
  - Tests rule role selection, revision changes, feature state transitions, and status derivation.
- Modify `database/tunnel/tunnel.go`
  - Uses control readiness in `BuildRuleStatusForUser`.
- Create `api/client/tunnel_control.go`
  - Owns WebSocket control handler, message shapes, hello validation, rule sync send, heartbeat/ack/status receive.
- Create `api/client/tunnel_control_test.go`
  - Tests pure message parsing and feature gate helper behavior.
- Modify `cmd/server.go`
  - Registers `tokenAuthrized.GET("/tunnel", client.WebSocketTunnelControl)`.

Agent repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`

- Modify `src/config.rs`
  - Adds `tunnel_control_enabled` with `AGENT_TUNNEL_CONTROL_ENABLED`.
- Modify `src/protocol.rs`
  - Adds `build_tunnel_control_ws_url`.
- Create `src/tunnel_control.rs`
  - Owns control messages, serde parsing, WebSocket connector, one-shot runtime loop, and no-fatal error policy helpers.
- Modify `src/lib.rs`
  - Exports `tunnel_control`.
- Modify `src/main.rs`
  - Starts tunnel control in a background thread when enabled and not `--once`; in `--once` it runs a non-fatal one-shot check.
- Create `tests/tunnel_control.rs`
  - Tests URL building, message encoding/decoding, rule sync validation, no-listener invariants, and runtime non-fatal behavior.
- Modify `tests/config.rs`
  - Tests tunnel control default-enabled and disable env parsing.
- Modify `tests/protocol.rs`
  - Tests tunnel control URL helper.

---

### Task 1: Backend Tunnel Control Model And Migration

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\models\tunnel.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\dbcore\dbcore.go`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\control_test.go`

- [ ] **Step 1: Write the failing model migration test**

Create `database/tunnel/control_test.go` with this initial test:

```go
package tunnel

import (
	"testing"

	"github.com/komari-monitor/komari/database/models"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
)

func newTunnelControlTestDB(t *testing.T) *gorm.DB {
	t.Helper()
	db, err := gorm.Open(sqlite.Open(t.TempDir()+"/tunnel-control.db"), &gorm.Config{})
	if err != nil {
		t.Fatalf("open test db: %v", err)
	}
	if err := db.AutoMigrate(&models.Client{}, &models.TunnelRule{}, &models.ClientTunnelState{}); err != nil {
		t.Fatalf("migrate test db: %v", err)
	}
	return db
}

func TestClientTunnelStateMigratesAndStoresCapability(t *testing.T) {
	db := newTunnelControlTestDB(t)

	state := models.ClientTunnelState{
		UserID:           "user-a",
		ClientUUID:       "node-a",
		Connected:        true,
		AgentVersion:     "0.1.0",
		ControlProtocol:  models.TunnelControlProtocolV1,
		CapabilitiesJSON: `["tunnel_control","rule_sync","status_report"]`,
	}
	if err := db.Create(&state).Error; err != nil {
		t.Fatalf("create state: %v", err)
	}

	var loaded models.ClientTunnelState
	if err := db.Where("client_uuid = ?", "node-a").First(&loaded).Error; err != nil {
		t.Fatalf("load state: %v", err)
	}
	if !loaded.Connected || loaded.ControlProtocol != models.TunnelControlProtocolV1 {
		t.Fatalf("unexpected tunnel state: %+v", loaded)
	}
}
```

- [ ] **Step 2: Run the failing model test**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./database/tunnel -run TestClientTunnelStateMigratesAndStoresCapability -count=1
```

Expected: FAIL with `undefined: models.ClientTunnelState` and `undefined: models.TunnelControlProtocolV1`.

- [ ] **Step 3: Add tunnel control constants and model**

Append to `database/models/tunnel.go` after `TunnelRule`:

```go
const (
	TunnelControlProtocolV1      = "keli-tunnel-control.v1"
	TunnelCapabilityControl      = "tunnel_control"
	TunnelCapabilityRuleSync     = "rule_sync"
	TunnelCapabilityStatusReport = "status_report"
)

type ClientTunnelState struct {
	ID               uint      `json:"id,omitempty" gorm:"primaryKey;autoIncrement"`
	UserID           string    `json:"user_id,omitempty" gorm:"type:varchar(36);not null;index:idx_client_tunnel_state_user_client,unique"`
	ClientUUID       string    `json:"client_uuid" gorm:"type:varchar(64);not null;index:idx_client_tunnel_state_user_client,unique;index"`
	Connected        bool      `json:"connected" gorm:"not null;default:false;index"`
	AgentVersion     string    `json:"agent_version" gorm:"type:varchar(64);not null;default:''"`
	ControlProtocol  string    `json:"control_protocol" gorm:"type:varchar(64);not null;default:'';index"`
	CapabilitiesJSON string    `json:"capabilities_json" gorm:"type:text;not null;default:'[]'"`
	LastRuleRevision string    `json:"last_rule_revision" gorm:"type:varchar(128);not null;default:''"`
	LastAckRevision  string    `json:"last_ack_revision" gorm:"type:varchar(128);not null;default:''"`
	LastHeartbeatAt  LocalTime `json:"last_heartbeat_at"`
	LastError        string    `json:"last_error" gorm:"type:text"`
	CreatedAt        LocalTime `json:"created_at"`
	UpdatedAt        LocalTime `json:"updated_at"`
}
```

- [ ] **Step 4: Add model migration**

In `database/dbcore/dbcore.go`, find the auto-migrate block that contains `&models.TunnelRule{}` and add:

```go
&models.ClientTunnelState{},
```

Keep it immediately after `&models.TunnelRule{}`.

- [ ] **Step 5: Run the model test**

Run:

```powershell
go test ./database/tunnel -run TestClientTunnelStateMigratesAndStoresCapability -count=1
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add database/models/tunnel.go database/dbcore/dbcore.go database/tunnel/control_test.go
git commit -m "Add tunnel control state model"
```

---

### Task 2: Backend Rule Selection, Revisioning, And State Service

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\control.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\control_test.go`

- [ ] **Step 1: Add failing service tests**

Append these helpers and tests to `database/tunnel/control_test.go`:

```go
func seedControlClient(t *testing.T, db *gorm.DB, userID, uuid, group string) {
	t.Helper()
	now := models.FromTime(time.Now())
	if err := db.Create(&models.Client{
		UUID:      uuid,
		Token:     uuid + "-token",
		UserID:    userID,
		Name:      uuid,
		Group:     group,
		CreatedAt: now,
		UpdatedAt: now,
	}).Error; err != nil {
		t.Fatalf("seed client %s: %v", uuid, err)
	}
}

func seedControlRule(t *testing.T, db *gorm.DB, userID, name, ingressGroup, egressGroup string, port int) models.TunnelRule {
	t.Helper()
	rule := models.TunnelRule{
		UserID:                userID,
		Name:                  name,
		Enabled:               true,
		Protocol:              models.TunnelProtocolTCP,
		IngressGroup:          ingressGroup,
		ListenAddress:         "0.0.0.0",
		ListenPort:            port,
		EgressGroup:           egressGroup,
		TargetHost:            "127.0.0.1",
		TargetPort:            3389,
		SourceAllowlist:       "0.0.0.0/0",
		MaxConcurrentSessions: 32,
		LastRevision:          1,
	}
	if err := db.Create(&rule).Error; err != nil {
		t.Fatalf("seed rule %s: %v", name, err)
	}
	return rule
}

func TestSelectRulesForClientAssignsIngressEgressAndBothRoles(t *testing.T) {
	db := newTunnelControlTestDB(t)
	seedControlClient(t, db, "user-a", "edge-a", "edge")
	seedControlClient(t, db, "user-a", "rdp-a", "rdp")
	seedControlClient(t, db, "user-a", "both-a", "both")
	seedControlRule(t, db, "user-a", "edge-to-rdp", "edge", "rdp", 10088)
	seedControlRule(t, db, "user-a", "rdp-to-edge", "rdp", "edge", 10089)
	seedControlRule(t, db, "user-a", "both-loop", "both", "both", 10090)

	edgeRules, err := selectRulesForClientWithDB(db, "user-a", "edge-a")
	if err != nil {
		t.Fatalf("select edge rules: %v", err)
	}
	if got := selectedRuleRoles(edgeRules); got != "1:ingress,2:egress" {
		t.Fatalf("unexpected edge roles: %s", got)
	}

	bothRules, err := selectRulesForClientWithDB(db, "user-a", "both-a")
	if err != nil {
		t.Fatalf("select both rules: %v", err)
	}
	if got := selectedRuleRoles(bothRules); got != "3:both" {
		t.Fatalf("unexpected both roles: %s", got)
	}
}

func TestRuleSetRevisionChangesOnRuleUpdateAndDelete(t *testing.T) {
	db := newTunnelControlTestDB(t)
	seedControlClient(t, db, "user-a", "edge-a", "edge")
	rule := seedControlRule(t, db, "user-a", "edge-to-rdp", "edge", "rdp", 10088)

	first, err := selectRulesForClientWithDB(db, "user-a", "edge-a")
	if err != nil {
		t.Fatalf("select first: %v", err)
	}
	firstRevision := ruleSetRevision(first)
	if firstRevision == "" {
		t.Fatal("expected non-empty revision")
	}

	if err := db.Model(&models.TunnelRule{}).Where("id = ?", rule.ID).Updates(map[string]any{
		"listen_port":   10089,
		"last_revision": 2,
	}).Error; err != nil {
		t.Fatalf("update rule: %v", err)
	}
	second, err := selectRulesForClientWithDB(db, "user-a", "edge-a")
	if err != nil {
		t.Fatalf("select second: %v", err)
	}
	if secondRevision := ruleSetRevision(second); secondRevision == firstRevision {
		t.Fatalf("expected revision to change after update, still %s", secondRevision)
	}

	if err := db.Delete(&models.TunnelRule{}, rule.ID).Error; err != nil {
		t.Fatalf("delete rule: %v", err)
	}
	third, err := selectRulesForClientWithDB(db, "user-a", "edge-a")
	if err != nil {
		t.Fatalf("select third: %v", err)
	}
	if deleteRevision := ruleSetRevision(third); deleteRevision == firstRevision {
		t.Fatalf("expected revision to change after delete, still %s", deleteRevision)
	}
}

func TestUpsertHeartbeatAndDisconnectTunnelState(t *testing.T) {
	db := newTunnelControlTestDB(t)
	err := upsertClientTunnelHelloWithDB(db, "user-a", "node-a", TunnelHello{
		ControlProtocol: models.TunnelControlProtocolV1,
		AgentVersion:    "0.1.0",
		Capabilities:    []string{models.TunnelCapabilityControl, models.TunnelCapabilityRuleSync, models.TunnelCapabilityStatusReport},
	})
	if err != nil {
		t.Fatalf("upsert hello: %v", err)
	}
	if err := recordClientTunnelHeartbeatWithDB(db, "user-a", "node-a", "rev-a", ""); err != nil {
		t.Fatalf("record heartbeat: %v", err)
	}

	var state models.ClientTunnelState
	if err := db.Where("client_uuid = ?", "node-a").First(&state).Error; err != nil {
		t.Fatalf("load state: %v", err)
	}
	if !state.Connected || state.LastRuleRevision != "rev-a" || state.LastHeartbeatAt.IsZero() {
		t.Fatalf("unexpected heartbeat state: %+v", state)
	}

	if err := markClientTunnelDisconnectedWithDB(db, "user-a", "node-a", "socket closed"); err != nil {
		t.Fatalf("mark disconnected: %v", err)
	}
	if err := db.Where("client_uuid = ?", "node-a").First(&state).Error; err != nil {
		t.Fatalf("reload state: %v", err)
	}
	if state.Connected || state.LastError != "socket closed" {
		t.Fatalf("unexpected disconnected state: %+v", state)
	}
}
```

Add imports to `database/tunnel/control_test.go`:

```go
import (
	"strings"
	"testing"
	"time"
)
```

- [ ] **Step 2: Run the failing service tests**

Run:

```powershell
go test ./database/tunnel -run "TestSelectRulesForClient|TestRuleSetRevision|TestUpsertHeartbeat" -count=1
```

Expected: FAIL with undefined helper and service functions.

- [ ] **Step 3: Add backend control service**

Create `database/tunnel/control.go`:

```go
package tunnel

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/komari-monitor/komari/database/dbcore"
	"github.com/komari-monitor/komari/database/models"
	"gorm.io/gorm"
)

const (
	TunnelRuleRoleIngress = "ingress"
	TunnelRuleRoleEgress  = "egress"
	TunnelRuleRoleBoth    = "both"
)

type TunnelHello struct {
	ControlProtocol string   `json:"control_protocol"`
	AgentVersion    string   `json:"agent_version"`
	Capabilities    []string `json:"capabilities"`
}

type SelectedRule struct {
	ID                    uint   `json:"id"`
	Name                  string `json:"name"`
	Enabled               bool   `json:"enabled"`
	Protocol              string `json:"protocol"`
	Role                  string `json:"role"`
	IngressGroup          string `json:"ingress_group"`
	ListenAddress         string `json:"listen_address"`
	ListenPort            int    `json:"listen_port"`
	EgressGroup           string `json:"egress_group"`
	TargetHost            string `json:"target_host"`
	TargetPort            int    `json:"target_port"`
	SourceAllowlist       string `json:"source_allowlist"`
	MaxConcurrentSessions int    `json:"max_concurrent_sessions"`
	LastRevision          int64  `json:"last_revision"`
}

func selectedRuleFromModel(rule models.TunnelRule, role string) SelectedRule {
	return SelectedRule{
		ID:                    rule.ID,
		Name:                  rule.Name,
		Enabled:               rule.Enabled,
		Protocol:              rule.Protocol,
		Role:                  role,
		IngressGroup:          rule.IngressGroup,
		ListenAddress:         rule.ListenAddress,
		ListenPort:            rule.ListenPort,
		EgressGroup:           rule.EgressGroup,
		TargetHost:            rule.TargetHost,
		TargetPort:            rule.TargetPort,
		SourceAllowlist:       rule.SourceAllowlist,
		MaxConcurrentSessions: rule.MaxConcurrentSessions,
		LastRevision:          rule.LastRevision,
	}
}

func selectRulesForClientWithDB(db *gorm.DB, userUUID, clientUUID string) ([]SelectedRule, error) {
	userUUID = strings.TrimSpace(userUUID)
	clientUUID = strings.TrimSpace(clientUUID)
	if userUUID == "" || clientUUID == "" {
		return nil, fmt.Errorf("user and client are required")
	}

	var client models.Client
	if err := db.Where("user_id = ? AND uuid = ?", userUUID, clientUUID).First(&client).Error; err != nil {
		return nil, err
	}
	group := strings.TrimSpace(client.Group)
	if group == "" {
		return []SelectedRule{}, nil
	}

	var rules []models.TunnelRule
	if err := db.Where("user_id = ? AND enabled = ? AND (ingress_group = ? OR egress_group = ?)", userUUID, true, group, group).
		Order("id ASC").
		Find(&rules).Error; err != nil {
		return nil, err
	}

	selected := make([]SelectedRule, 0, len(rules))
	for _, rule := range rules {
		role := ""
		if rule.IngressGroup == group && rule.EgressGroup == group {
			role = TunnelRuleRoleBoth
		} else if rule.IngressGroup == group {
			role = TunnelRuleRoleIngress
		} else if rule.EgressGroup == group {
			role = TunnelRuleRoleEgress
		}
		if role != "" {
			selected = append(selected, selectedRuleFromModel(rule, role))
		}
	}
	return selected, nil
}

func SelectRulesForClient(userUUID, clientUUID string) ([]SelectedRule, string, error) {
	rules, err := selectRulesForClientWithDB(dbcore.GetDBInstance(), userUUID, clientUUID)
	if err != nil {
		return nil, "", err
	}
	return rules, ruleSetRevision(rules), nil
}

func ruleSetRevision(rules []SelectedRule) string {
	canonical := append([]SelectedRule(nil), rules...)
	sort.SliceStable(canonical, func(i, j int) bool {
		if canonical[i].ID == canonical[j].ID {
			return canonical[i].Role < canonical[j].Role
		}
		return canonical[i].ID < canonical[j].ID
	})
	payload, _ := json.Marshal(canonical)
	sum := sha256.Sum256(payload)
	return hex.EncodeToString(sum[:])
}

func selectedRuleRoles(rules []SelectedRule) string {
	parts := make([]string, 0, len(rules))
	for _, rule := range rules {
		parts = append(parts, fmt.Sprintf("%d:%s", rule.ID, rule.Role))
	}
	sort.Strings(parts)
	return strings.Join(parts, ",")
}

func upsertClientTunnelHelloWithDB(db *gorm.DB, userUUID, clientUUID string, hello TunnelHello) error {
	capabilities, err := json.Marshal(hello.Capabilities)
	if err != nil {
		return err
	}
	now := models.FromTime(time.Now())
	state := models.ClientTunnelState{
		UserID:            strings.TrimSpace(userUUID),
		ClientUUID:        strings.TrimSpace(clientUUID),
		Connected:         true,
		AgentVersion:      strings.TrimSpace(hello.AgentVersion),
		ControlProtocol:   strings.TrimSpace(hello.ControlProtocol),
		CapabilitiesJSON:  string(capabilities),
		LastHeartbeatAt:   now,
		LastError:         "",
	}
	var existing models.ClientTunnelState
	err = db.Where("user_id = ? AND client_uuid = ?", state.UserID, state.ClientUUID).First(&existing).Error
	if err == nil {
		return db.Model(&models.ClientTunnelState{}).Where("id = ?", existing.ID).Updates(map[string]any{
			"connected":         true,
			"agent_version":     state.AgentVersion,
			"control_protocol":  state.ControlProtocol,
			"capabilities_json": state.CapabilitiesJSON,
			"last_heartbeat_at": now,
			"last_error":        "",
		}).Error
	}
	if !errors.Is(err, gorm.ErrRecordNotFound) {
		return err
	}
	return db.Create(&state).Error
}

func UpsertClientTunnelHello(userUUID, clientUUID string, hello TunnelHello) error {
	return upsertClientTunnelHelloWithDB(dbcore.GetDBInstance(), userUUID, clientUUID, hello)
}

func recordClientTunnelHeartbeatWithDB(db *gorm.DB, userUUID, clientUUID, revision, lastError string) error {
	return db.Model(&models.ClientTunnelState{}).
		Where("user_id = ? AND client_uuid = ?", strings.TrimSpace(userUUID), strings.TrimSpace(clientUUID)).
		Updates(map[string]any{
			"connected":          true,
			"last_rule_revision": strings.TrimSpace(revision),
			"last_heartbeat_at":  models.FromTime(time.Now()),
			"last_error":         strings.TrimSpace(lastError),
		}).Error
}

func RecordClientTunnelHeartbeat(userUUID, clientUUID, revision, lastError string) error {
	return recordClientTunnelHeartbeatWithDB(dbcore.GetDBInstance(), userUUID, clientUUID, revision, lastError)
}

func MarkClientTunnelAck(userUUID, clientUUID, revision string) error {
	return dbcore.GetDBInstance().Model(&models.ClientTunnelState{}).
		Where("user_id = ? AND client_uuid = ?", strings.TrimSpace(userUUID), strings.TrimSpace(clientUUID)).
		Updates(map[string]any{"last_ack_revision": strings.TrimSpace(revision), "connected": true}).Error
}

func markClientTunnelDisconnectedWithDB(db *gorm.DB, userUUID, clientUUID, reason string) error {
	return db.Model(&models.ClientTunnelState{}).
		Where("user_id = ? AND client_uuid = ?", strings.TrimSpace(userUUID), strings.TrimSpace(clientUUID)).
		Updates(map[string]any{"connected": false, "last_error": strings.TrimSpace(reason)}).Error
}

func MarkClientTunnelDisconnected(userUUID, clientUUID, reason string) error {
	return markClientTunnelDisconnectedWithDB(dbcore.GetDBInstance(), userUUID, clientUUID, reason)
}
```

Also add `errors` to the import list.

- [ ] **Step 4: Run service tests**

Run:

```powershell
go test ./database/tunnel -run "TestSelectRulesForClient|TestRuleSetRevision|TestUpsertHeartbeat" -count=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```powershell
git add database/tunnel/control.go database/tunnel/control_test.go
git commit -m "Add tunnel control rule sync service"
```

---

### Task 3: Backend Control-Aware Rule Status

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\control.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\tunnel.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\control_test.go`

- [ ] **Step 1: Add failing status tests**

Append to `database/tunnel/control_test.go`:

```go
func TestControlAwareRuleStatusDistinguishesUnsupportedDisconnectedAndReady(t *testing.T) {
	db := newTunnelControlTestDB(t)
	seedControlClient(t, db, "user-a", "edge-a", "edge")
	seedControlClient(t, db, "user-a", "rdp-a", "rdp")
	rule := seedControlRule(t, db, "user-a", "edge-to-rdp", "edge", "rdp", 10088)

	status, err := BuildRuleStatusForUser(db, "user-a", rule)
	if err != nil {
		t.Fatalf("status without capability: %v", err)
	}
	if status != models.TunnelStatusUnsupportedAgent {
		t.Fatalf("expected unsupported_agent, got %s", status)
	}

	for _, clientUUID := range []string{"edge-a", "rdp-a"} {
		if err := upsertClientTunnelHelloWithDB(db, "user-a", clientUUID, TunnelHello{
			ControlProtocol: models.TunnelControlProtocolV1,
			AgentVersion:    "0.1.0",
			Capabilities:    []string{models.TunnelCapabilityControl, models.TunnelCapabilityRuleSync, models.TunnelCapabilityStatusReport},
		}); err != nil {
			t.Fatalf("upsert hello %s: %v", clientUUID, err)
		}
		if err := markClientTunnelDisconnectedWithDB(db, "user-a", clientUUID, "not connected"); err != nil {
			t.Fatalf("disconnect %s: %v", clientUUID, err)
		}
	}

	status, err = BuildRuleStatusForUser(db, "user-a", rule)
	if err != nil {
		t.Fatalf("status disconnected: %v", err)
	}
	if status != models.TunnelStatusRelayUnavailable {
		t.Fatalf("expected relay_unavailable, got %s", status)
	}

	for _, clientUUID := range []string{"edge-a", "rdp-a"} {
		if err := upsertClientTunnelHelloWithDB(db, "user-a", clientUUID, TunnelHello{
			ControlProtocol: models.TunnelControlProtocolV1,
			AgentVersion:    "0.1.0",
			Capabilities:    []string{models.TunnelCapabilityControl, models.TunnelCapabilityRuleSync, models.TunnelCapabilityStatusReport},
		}); err != nil {
			t.Fatalf("reconnect %s: %v", clientUUID, err)
		}
	}

	status, err = BuildRuleStatusForUser(db, "user-a", rule)
	if err != nil {
		t.Fatalf("status ready: %v", err)
	}
	if status != models.TunnelStatusOK {
		t.Fatalf("expected ok, got %s", status)
	}
}
```

- [ ] **Step 2: Run the failing status test**

Run:

```powershell
go test ./database/tunnel -run TestControlAwareRuleStatusDistinguishesUnsupportedDisconnectedAndReady -count=1
```

Expected: FAIL because `BuildRuleStatusForUser` still returns `ok` when groups are non-empty.

- [ ] **Step 3: Add readiness helper**

Append to `database/tunnel/control.go`:

```go
type groupTunnelReadiness struct {
	HasClients         bool
	HasCapableClients  bool
	HasConnectedClient bool
}

func groupTunnelReadinessWithDB(db *gorm.DB, userUUID, group string) (groupTunnelReadiness, error) {
	var clients []models.Client
	if err := db.Where("user_id = ? AND `group` = ?", strings.TrimSpace(userUUID), strings.TrimSpace(group)).
		Find(&clients).Error; err != nil {
		return groupTunnelReadiness{}, err
	}
	ready := groupTunnelReadiness{HasClients: len(clients) > 0}
	for _, client := range clients {
		var state models.ClientTunnelState
		err := db.Where("user_id = ? AND client_uuid = ?", userUUID, client.UUID).First(&state).Error
		if errors.Is(err, gorm.ErrRecordNotFound) {
			continue
		}
		if err != nil {
			return groupTunnelReadiness{}, err
		}
		if state.ControlProtocol == models.TunnelControlProtocolV1 && strings.Contains(state.CapabilitiesJSON, models.TunnelCapabilityControl) {
			ready.HasCapableClients = true
			if state.Connected {
				ready.HasConnectedClient = true
			}
		}
	}
	return ready, nil
}
```

- [ ] **Step 4: Update status calculation**

In `database/tunnel/tunnel.go`, after the existing empty group checks in `BuildRuleStatusForUser`, add:

```go
ingressReady, err := groupTunnelReadinessWithDB(db, normalizedUserID, rule.IngressGroup)
if err != nil {
	return "", err
}
egressReady, err := groupTunnelReadinessWithDB(db, normalizedUserID, rule.EgressGroup)
if err != nil {
	return "", err
}
if !ingressReady.HasCapableClients || !egressReady.HasCapableClients {
	return models.TunnelStatusUnsupportedAgent, nil
}
if !ingressReady.HasConnectedClient || !egressReady.HasConnectedClient {
	return models.TunnelStatusRelayUnavailable, nil
}
```

Keep the existing `LastError` partial check after these readiness checks.

- [ ] **Step 5: Run tunnel database tests**

Run:

```powershell
go test ./database/tunnel -count=1
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add database/tunnel/control.go database/tunnel/tunnel.go database/tunnel/control_test.go
git commit -m "Use tunnel control state in rule status"
```

---

### Task 4: Backend Tunnel Control WebSocket Endpoint

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\client\tunnel_control.go`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\client\tunnel_control_test.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\cmd\server.go`

- [ ] **Step 1: Add failing message tests**

Create `api/client/tunnel_control_test.go`:

```go
package client

import (
	"encoding/json"
	"testing"

	"github.com/komari-monitor/komari/database/models"
	tunneldb "github.com/komari-monitor/komari/database/tunnel"
)

func TestParseTunnelControlMessageParsesHelloHeartbeatAndAck(t *testing.T) {
	hello, err := parseTunnelControlMessage([]byte(`{
		"type":"hello",
		"control_protocol":"keli-tunnel-control.v1",
		"agent_version":"0.1.0",
		"capabilities":["tunnel_control","rule_sync","status_report"],
		"data_plane":false
	}`))
	if err != nil {
		t.Fatalf("parse hello: %v", err)
	}
	if hello.Type != tunnelControlMessageHello || hello.ControlProtocol != models.TunnelControlProtocolV1 {
		t.Fatalf("unexpected hello: %+v", hello)
	}

	heartbeat, err := parseTunnelControlMessage([]byte(`{"type":"heartbeat","last_rule_revision":"rev-a","active_rules":[]}`))
	if err != nil {
		t.Fatalf("parse heartbeat: %v", err)
	}
	if heartbeat.Type != tunnelControlMessageHeartbeat || heartbeat.LastRuleRevision != "rev-a" {
		t.Fatalf("unexpected heartbeat: %+v", heartbeat)
	}

	ack, err := parseTunnelControlMessage([]byte(`{"type":"rule_ack","revision":"rev-b","accepted_rule_ids":[1],"rejected_rules":[]}`))
	if err != nil {
		t.Fatalf("parse ack: %v", err)
	}
	if ack.Type != tunnelControlMessageRuleAck || ack.Revision != "rev-b" || len(ack.AcceptedRuleIDs) != 1 {
		t.Fatalf("unexpected ack: %+v", ack)
	}
}

func TestBuildTunnelRuleSyncPayloadUsesSelectedRules(t *testing.T) {
	rules := []tunneldb.SelectedRule{{
		ID:                    7,
		Name:                  "RDP",
		Enabled:               true,
		Protocol:              models.TunnelProtocolTCP,
		Role:                  tunneldb.TunnelRuleRoleIngress,
		IngressGroup:          "edge",
		ListenAddress:         "0.0.0.0",
		ListenPort:            10088,
		EgressGroup:           "rdp",
		TargetHost:            "127.0.0.1",
		TargetPort:            3389,
		SourceAllowlist:       "0.0.0.0/0",
		MaxConcurrentSessions: 32,
		LastRevision:          1,
	}}
	payload := buildTunnelRuleSyncPayload("rev-a", rules)
	bytes, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	if string(bytes) == "" || payload.Type != tunnelControlMessageRuleSync || payload.Revision != "rev-a" {
		t.Fatalf("unexpected payload: %+v", payload)
	}
	if len(payload.Rules) != 1 || payload.Rules[0].Role != tunneldb.TunnelRuleRoleIngress {
		t.Fatalf("unexpected rules: %+v", payload.Rules)
	}
}
```

- [ ] **Step 2: Run the failing message tests**

Run:

```powershell
go test ./api/client -run TestParseTunnelControlMessage -count=1
```

Expected: FAIL because parser and payload helpers do not exist.

- [ ] **Step 3: Add WebSocket handler and helpers**

Create `api/client/tunnel_control.go`:

```go
package client

import (
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/gorilla/websocket"
	"github.com/komari-monitor/komari/api"
	"github.com/komari-monitor/komari/config"
	tunneldb "github.com/komari-monitor/komari/database/tunnel"
)

const (
	tunnelControlMessageHello     = "hello"
	tunnelControlMessageHelloAck  = "hello_ack"
	tunnelControlMessageHeartbeat = "heartbeat"
	tunnelControlMessageRuleSync  = "rule_sync"
	tunnelControlMessageRuleAck   = "rule_ack"
	tunnelControlMessageRuleStatus = "rule_status"
	tunnelControlMessageError     = "error"
	tunnelControlReadWait         = 45 * time.Second
	tunnelControlHeartbeatSeconds = 15
)

type tunnelControlMessage struct {
	Type                 string                 `json:"type"`
	ControlProtocol      string                 `json:"control_protocol,omitempty"`
	AgentVersion         string                 `json:"agent_version,omitempty"`
	Capabilities         []string               `json:"capabilities,omitempty"`
	DataPlane            bool                   `json:"data_plane,omitempty"`
	LastRuleRevision     string                 `json:"last_rule_revision,omitempty"`
	ActiveRules          []uint                 `json:"active_rules,omitempty"`
	Revision             string                 `json:"revision,omitempty"`
	AcceptedRuleIDs      []uint                 `json:"accepted_rule_ids,omitempty"`
	RejectedRules        []tunnelRejectedRule   `json:"rejected_rules,omitempty"`
	Rules                []tunneldb.SelectedRule `json:"rules,omitempty"`
	ServerProtocol        string                 `json:"server_protocol,omitempty"`
	HeartbeatIntervalSec int                    `json:"heartbeat_interval_seconds,omitempty"`
	Code                 string                 `json:"code,omitempty"`
	Message              string                 `json:"message,omitempty"`
}

type tunnelRejectedRule struct {
	ID    uint   `json:"id"`
	Error string `json:"error"`
}

func parseTunnelControlMessage(bytes []byte) (tunnelControlMessage, error) {
	var message tunnelControlMessage
	if err := json.Unmarshal(bytes, &message); err != nil {
		return tunnelControlMessage{}, err
	}
	message.Type = strings.TrimSpace(message.Type)
	if message.Type == "" {
		return tunnelControlMessage{}, errors.New("message type is required")
	}
	return message, nil
}

func buildTunnelRuleSyncPayload(revision string, rules []tunneldb.SelectedRule) tunnelControlMessage {
	return tunnelControlMessage{
		Type:     tunnelControlMessageRuleSync,
		Revision: strings.TrimSpace(revision),
		Rules:    rules,
	}
}

func writeTunnelControlError(conn *websocket.Conn, code, message string) {
	_ = conn.WriteJSON(tunnelControlMessage{
		Type:    tunnelControlMessageError,
		Code:    code,
		Message: message,
	})
}

func currentTunnelClientScope(c *gin.Context) (string, string, bool) {
	clientUUID, _ := c.Get("client_uuid")
	userID, _ := c.Get("user_id")
	clientUUIDString, _ := clientUUID.(string)
	userIDString, _ := userID.(string)
	clientUUIDString = strings.TrimSpace(clientUUIDString)
	userIDString = strings.TrimSpace(userIDString)
	return userIDString, clientUUIDString, userIDString != "" && clientUUIDString != ""
}

func WebSocketTunnelControl(c *gin.Context) {
	if !websocket.IsWebSocketUpgrade(c.Request) {
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "Require WebSocket upgrade"})
		return
	}
	userID, clientUUID, ok := currentTunnelClientScope(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"status": "error", "error": "invalid token"})
		return
	}
	allowed, err := config.IsUserFeatureAllowed(userID, config.UserFeatureTunnels)
	if err != nil || !allowed {
		upgrader := websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return true }}
		conn, upgradeErr := upgrader.Upgrade(c.Writer, c.Request, nil)
		if upgradeErr == nil {
			writeTunnelControlError(conn, "feature_disabled", "tunnel forwarding is disabled for this account")
			_ = conn.Close()
		}
		return
	}

	upgrader := websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return true }}
	conn, err := upgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "Failed to upgrade to WebSocket."+err.Error()})
		return
	}
	defer conn.Close()
	defer func() {
		if err := tunneldb.MarkClientTunnelDisconnected(userID, clientUUID, "control socket closed"); err != nil {
			log.Printf("mark tunnel control disconnected %s: %v", clientUUID, err)
		}
	}()

	conn.SetReadDeadline(time.Now().Add(tunnelControlReadWait))
	_, bytes, err := conn.ReadMessage()
	if err != nil {
		return
	}
	first, err := parseTunnelControlMessage(bytes)
	if err != nil || first.Type != tunnelControlMessageHello {
		writeTunnelControlError(conn, "invalid_hello", "first tunnel control message must be hello")
		return
	}
	if err := tunneldb.UpsertClientTunnelHello(userID, clientUUID, tunneldb.TunnelHello{
		ControlProtocol: first.ControlProtocol,
		AgentVersion:    first.AgentVersion,
		Capabilities:    first.Capabilities,
	}); err != nil {
		writeTunnelControlError(conn, "state_error", err.Error())
		return
	}
	if err := conn.WriteJSON(tunnelControlMessage{
		Type:                 tunnelControlMessageHelloAck,
		ServerProtocol:       first.ControlProtocol,
		HeartbeatIntervalSec: tunnelControlHeartbeatSeconds,
	}); err != nil {
		return
	}
	if err := sendTunnelRuleSync(conn, userID, clientUUID); err != nil {
		writeTunnelControlError(conn, "rule_sync_failed", err.Error())
	}

	for {
		conn.SetReadDeadline(time.Now().Add(tunnelControlReadWait))
		_, bytes, err := conn.ReadMessage()
		if err != nil {
			return
		}
		message, err := parseTunnelControlMessage(bytes)
		if err != nil {
			writeTunnelControlError(conn, "invalid_message", err.Error())
			continue
		}
		switch message.Type {
		case tunnelControlMessageHeartbeat:
			_ = tunneldb.RecordClientTunnelHeartbeat(userID, clientUUID, message.LastRuleRevision, "")
		case tunnelControlMessageRuleAck:
			_ = tunneldb.MarkClientTunnelAck(userID, clientUUID, message.Revision)
		case tunnelControlMessageRuleStatus:
			_ = tunneldb.RecordClientTunnelHeartbeat(userID, clientUUID, message.Revision, "")
		default:
			writeTunnelControlError(conn, "unknown_message", "unknown tunnel control message")
		}
	}
}

func sendTunnelRuleSync(conn *websocket.Conn, userID, clientUUID string) error {
	rules, revision, err := tunneldb.SelectRulesForClient(userID, clientUUID)
	if err != nil {
		return err
	}
	return conn.WriteJSON(buildTunnelRuleSyncPayload(revision, rules))
}
```

Keep long lines wrapped if `gofmt` changes them.

- [ ] **Step 4: Register route**

In `cmd/server.go`, inside the `tokenAuthrized` group near existing client routes, add:

```go
tokenAuthrized.GET("/tunnel", client.WebSocketTunnelControl)
```

- [ ] **Step 5: Run API tests**

Run:

```powershell
go test ./api/client -run TestParseTunnelControlMessage -count=1
go test ./database/tunnel -count=1
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add api/client/tunnel_control.go api/client/tunnel_control_test.go cmd/server.go
git commit -m "Add tunnel control websocket endpoint"
```

---

### Task 5: Agent Protocol URL And Config

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\protocol.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\config.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\protocol.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\config.rs`

- [ ] **Step 1: Add failing protocol and config tests**

Append to `tests/protocol.rs`:

```rust
#[test]
fn tunnel_control_ws_url_uses_separate_endpoint_and_escapes_token() {
    let url = build_tunnel_control_ws_url("https://panel.example.com/base/", "token with/slash").unwrap();

    assert_eq!(
        url,
        "wss://panel.example.com/base/api/clients/tunnel?token=token%20with%2Fslash"
    );
}
```

Update the import at the top of `tests/protocol.rs`:

```rust
use kelicloud_agent_rs::protocol::{
    build_report_ws_url, build_terminal_ws_url, build_tunnel_control_ws_url, parse_backend_message,
    BackendMessage,
};
```

Append to `tests/config.rs`:

```rust
#[test]
fn tunnel_control_is_enabled_by_default_and_can_be_disabled() {
    let config = AgentConfig::from_args_and_env(
        ["agent", "--endpoint", "https://panel.example.com", "--token", "token"],
        |_| None,
    )
    .unwrap();
    assert!(config.tunnel_control_enabled);

    let disabled = AgentConfig::from_args_and_env(
        ["agent", "--endpoint", "https://panel.example.com", "--token", "token"],
        |key| (key == "AGENT_TUNNEL_CONTROL_ENABLED").then(|| "false".to_string()),
    )
    .unwrap();
    assert!(!disabled.tunnel_control_enabled);
}
```

- [ ] **Step 2: Run failing agent protocol/config tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`:

```powershell
cargo test tunnel_control --test protocol --test config
```

Expected: FAIL with undefined `build_tunnel_control_ws_url` and missing `tunnel_control_enabled`.

- [ ] **Step 3: Add protocol URL helper**

In `src/protocol.rs`, add after `build_report_ws_url`:

```rust
pub fn build_tunnel_control_ws_url(endpoint: &str, token: &str) -> Result<String, ProtocolError> {
    let token = require_non_empty(token, ProtocolError::EmptyToken)?;
    build_ws_url(endpoint, "/api/clients/tunnel", &[("token", token)])
}
```

- [ ] **Step 4: Add config field and parsing**

In `src/config.rs`, add a field to `AgentConfig`:

```rust
pub tunnel_control_enabled: bool,
```

Initialize it in `from_args_and_env` near the other booleans:

```rust
let mut tunnel_control_enabled = env_lookup("AGENT_TUNNEL_CONTROL_ENABLED")
    .as_deref()
    .map(parse_tunnel_control_enabled)
    .unwrap_or(true);
```

Add CLI flags in the argument match:

```rust
"--disable-tunnel-control" => {
    tunnel_control_enabled = false;
}
"--enable-tunnel-control" => {
    tunnel_control_enabled = true;
}
```

Apply env override after other env applications:

```rust
if let Some(value) = env_lookup("AGENT_TUNNEL_CONTROL_ENABLED") {
    tunnel_control_enabled = parse_tunnel_control_enabled(&value);
}
```

Add file config field:

```rust
tunnel_control_enabled: Option<bool>,
```

Apply it after `host_proc`:

```rust
if let Some(value) = file_config.tunnel_control_enabled {
    tunnel_control_enabled = value;
}
```

Include it in the returned `AgentConfig`:

```rust
tunnel_control_enabled,
```

Add helper near `parse_bool`:

```rust
fn parse_tunnel_control_enabled(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "disabled" | "off" | "no"
    )
}
```

Update every `AgentConfig` literal in tests by adding:

```rust
tunnel_control_enabled: true,
```

- [ ] **Step 5: Run agent protocol/config tests**

Run:

```powershell
cargo test tunnel_control --test protocol --test config
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add src/protocol.rs src/config.rs tests/protocol.rs tests/config.rs
git commit -m "Add tunnel control agent config and URL"
```

---

### Task 6: Agent Tunnel Control Message Model

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\tunnel_control.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\lib.rs`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\tunnel_control.rs`

- [ ] **Step 1: Add failing message model tests**

Create `tests/tunnel_control.rs`:

```rust
use kelicloud_agent_rs::tunnel_control::{
    build_heartbeat, build_hello, build_rule_ack, parse_server_message, SelectedTunnelRule,
    TunnelControlClientMessage, TunnelControlServerMessage,
};

#[test]
fn builds_hello_without_data_plane_capability() {
    let hello = build_hello("0.1.0");
    let json = serde_json::to_string(&hello).unwrap();

    assert!(json.contains(r#""type":"hello""#));
    assert!(json.contains(r#""control_protocol":"keli-tunnel-control.v1""#));
    assert!(json.contains(r#""data_plane":false"#));
    assert!(json.contains("tunnel_control"));
}

#[test]
fn parses_rule_sync_and_preserves_role() {
    let message = parse_server_message(
        br#"{
            "type":"rule_sync",
            "revision":"rev-a",
            "rules":[{
                "id":7,
                "name":"RDP",
                "enabled":true,
                "protocol":"tcp",
                "role":"both",
                "ingress_group":"edge",
                "listen_address":"0.0.0.0",
                "listen_port":10088,
                "egress_group":"edge",
                "target_host":"127.0.0.1",
                "target_port":3389,
                "source_allowlist":"0.0.0.0/0",
                "max_concurrent_sessions":32,
                "last_revision":1
            }]
        }"#,
    )
    .unwrap();

    match message {
        TunnelControlServerMessage::RuleSync { revision, rules } => {
            assert_eq!(revision, "rev-a");
            assert_eq!(rules[0].role, "both");
            assert_eq!(rules[0].listen_port, 10088);
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn builds_rule_ack_and_heartbeat() {
    let ack = build_rule_ack("rev-a", &[SelectedTunnelRule::minimal(7)], &[]);
    let heartbeat = build_heartbeat("rev-a", &[7]);

    assert!(matches!(ack, TunnelControlClientMessage::RuleAck { .. }));
    assert!(matches!(heartbeat, TunnelControlClientMessage::Heartbeat { .. }));
    assert!(serde_json::to_string(&ack).unwrap().contains(r#""accepted_rule_ids":[7]"#));
    assert!(serde_json::to_string(&heartbeat).unwrap().contains(r#""last_rule_revision":"rev-a""#));
}
```

- [ ] **Step 2: Run failing message model tests**

Run:

```powershell
cargo test --test tunnel_control
```

Expected: FAIL because `tunnel_control` module does not exist.

- [ ] **Step 3: Add message model**

Create `src/tunnel_control.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

pub const TUNNEL_CONTROL_PROTOCOL_V1: &str = "keli-tunnel-control.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelControlError {
    InvalidMessage(String),
}

impl fmt::Display for TunnelControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMessage(message) => write!(f, "invalid tunnel control message: {message}"),
        }
    }
}

impl Error for TunnelControlError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelControlClientMessage {
    Hello {
        control_protocol: String,
        agent_version: String,
        capabilities: Vec<String>,
        data_plane: bool,
    },
    Heartbeat {
        last_rule_revision: String,
        active_rules: Vec<u32>,
    },
    RuleAck {
        revision: String,
        accepted_rule_ids: Vec<u32>,
        rejected_rules: Vec<RejectedTunnelRule>,
    },
    RuleStatus {
        revision: String,
        rules: Vec<TunnelRuleStatus>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RejectedTunnelRule {
    pub id: u32,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelRuleStatus {
    pub id: u32,
    pub status: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelControlServerMessage {
    HelloAck {
        server_protocol: String,
        heartbeat_interval_seconds: u64,
    },
    RuleSync {
        revision: String,
        rules: Vec<SelectedTunnelRule>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectedTunnelRule {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub protocol: String,
    pub role: String,
    pub ingress_group: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub egress_group: String,
    pub target_host: String,
    pub target_port: u16,
    pub source_allowlist: String,
    pub max_concurrent_sessions: u32,
    pub last_revision: i64,
}

impl SelectedTunnelRule {
    pub fn minimal(id: u32) -> Self {
        Self {
            id,
            name: "RDP".to_string(),
            enabled: true,
            protocol: "tcp".to_string(),
            role: "ingress".to_string(),
            ingress_group: "edge".to_string(),
            listen_address: "0.0.0.0".to_string(),
            listen_port: 10088,
            egress_group: "rdp".to_string(),
            target_host: "127.0.0.1".to_string(),
            target_port: 3389,
            source_allowlist: "0.0.0.0/0".to_string(),
            max_concurrent_sessions: 32,
            last_revision: 1,
        }
    }
}

pub fn build_hello(agent_version: &str) -> TunnelControlClientMessage {
    TunnelControlClientMessage::Hello {
        control_protocol: TUNNEL_CONTROL_PROTOCOL_V1.to_string(),
        agent_version: agent_version.trim().to_string(),
        capabilities: vec![
            "tunnel_control".to_string(),
            "rule_sync".to_string(),
            "status_report".to_string(),
        ],
        data_plane: false,
    }
}

pub fn build_heartbeat(revision: &str, active_rules: &[u32]) -> TunnelControlClientMessage {
    TunnelControlClientMessage::Heartbeat {
        last_rule_revision: revision.trim().to_string(),
        active_rules: active_rules.to_vec(),
    }
}

pub fn build_rule_ack(
    revision: &str,
    accepted_rules: &[SelectedTunnelRule],
    rejected_rules: &[RejectedTunnelRule],
) -> TunnelControlClientMessage {
    TunnelControlClientMessage::RuleAck {
        revision: revision.trim().to_string(),
        accepted_rule_ids: accepted_rules.iter().map(|rule| rule.id).collect(),
        rejected_rules: rejected_rules.to_vec(),
    }
}

pub fn parse_server_message(bytes: &[u8]) -> Result<TunnelControlServerMessage, TunnelControlError> {
    serde_json::from_slice(bytes).map_err(|error| TunnelControlError::InvalidMessage(error.to_string()))
}
```

In `src/lib.rs`, add:

```rust
pub mod tunnel_control;
```

- [ ] **Step 4: Run message model tests**

Run:

```powershell
cargo test --test tunnel_control
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```powershell
git add src/tunnel_control.rs src/lib.rs tests/tunnel_control.rs
git commit -m "Add tunnel control message model"
```

---

### Task 7: Agent Tunnel Control Runtime

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\tunnel_control.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\tunnel_control.rs`

- [ ] **Step 1: Add failing runtime tests**

Append to `tests/tunnel_control.rs`:

```rust
use kelicloud_agent_rs::transport::{HeaderPair, TransportError};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn tunnel_control_once_acks_rule_sync_and_sends_heartbeat() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelControlTransport::new(
        events.clone(),
        vec![
            br#"{"type":"hello_ack","server_protocol":"keli-tunnel-control.v1","heartbeat_interval_seconds":15}"#.to_vec(),
            br#"{"type":"rule_sync","revision":"rev-a","rules":[{"id":7,"name":"RDP","enabled":true,"protocol":"tcp","role":"ingress","ingress_group":"edge","listen_address":"0.0.0.0","listen_port":10088,"egress_group":"rdp","target_host":"127.0.0.1","target_port":3389,"source_allowlist":"0.0.0.0/0","max_concurrent_sessions":32,"last_revision":1}]}"#.to_vec(),
        ],
    );

    run_tunnel_control_once(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        &[],
        "0.1.0",
        &mut transport,
    )
    .unwrap();

    assert_eq!(events.borrow()[0], "connect:wss://panel.example.com/api/clients/tunnel?token=secret");
    assert!(events.borrow().iter().any(|event| event.contains(r#""type":"hello""#)));
    assert!(events.borrow().iter().any(|event| event.contains(r#""type":"rule_ack""#)));
    assert!(events.borrow().iter().any(|event| event.contains(r#""type":"heartbeat""#)));
}

#[test]
fn tunnel_control_unsupported_endpoint_is_non_fatal() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelControlTransport::new(events, Vec::new())
        .with_connect_error(TransportError::RequestFailed("HTTP 404".to_string()));

    let result = run_tunnel_control_once(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        &[],
        "0.1.0",
        &mut transport,
    );

    assert!(result.is_ok());
}
```

Add fake transport at the bottom of `tests/tunnel_control.rs`:

```rust
struct FakeTunnelControlTransport {
    events: Rc<RefCell<Vec<String>>>,
    inbound: Vec<Vec<u8>>,
    connect_error: Option<TransportError>,
}

impl FakeTunnelControlTransport {
    fn new(events: Rc<RefCell<Vec<String>>>, inbound: Vec<Vec<u8>>) -> Self {
        Self { events, inbound, connect_error: None }
    }

    fn with_connect_error(mut self, error: TransportError) -> Self {
        self.connect_error = Some(error);
        self
    }
}

impl TunnelControlTransport for FakeTunnelControlTransport {
    type Socket = FakeTunnelControlSocket;

    fn connect_tunnel_control(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        self.events.borrow_mut().push(format!("connect:{url}"));
        if let Some(error) = self.connect_error.take() {
            return Err(error);
        }
        Ok(FakeTunnelControlSocket {
            events: self.events.clone(),
            inbound: self.inbound.drain(..).collect(),
        })
    }
}

struct FakeTunnelControlSocket {
    events: Rc<RefCell<Vec<String>>>,
    inbound: Vec<Vec<u8>>,
}

impl TunnelControlSocket for FakeTunnelControlSocket {
    fn send_message(&mut self, message: &TunnelControlClientMessage) -> Result<(), TransportError> {
        self.events
            .borrow_mut()
            .push(serde_json::to_string(message).unwrap());
        Ok(())
    }

    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        if self.inbound.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.inbound.remove(0)))
    }
}
```

Update test imports:

```rust
use kelicloud_agent_rs::tunnel_control::{
    build_heartbeat, build_hello, build_rule_ack, parse_server_message, run_tunnel_control_once,
    SelectedTunnelRule, TunnelControlClientMessage, TunnelControlServerMessage,
    TunnelControlSocket, TunnelControlTransport,
};
```

- [ ] **Step 2: Run failing runtime tests**

Run:

```powershell
cargo test --test tunnel_control tunnel_control_once
```

Expected: FAIL with undefined runtime traits/functions.

- [ ] **Step 3: Add runtime traits and one-shot loop**

Append to `src/tunnel_control.rs`:

```rust
use crate::transport::{HeaderPair, TransportError};

pub trait TunnelControlSocket {
    fn send_message(&mut self, message: &TunnelControlClientMessage) -> Result<(), TransportError>;
    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
}

pub trait TunnelControlTransport {
    type Socket: TunnelControlSocket;

    fn connect_tunnel_control(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
}

pub fn is_non_fatal_tunnel_control_error(error: &TransportError) -> bool {
    match error {
        TransportError::InvalidClientToken { .. } => false,
        TransportError::EmptyEndpoint | TransportError::EmptyToken | TransportError::UnsupportedScheme(_) => false,
        TransportError::RequestFailed(message) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("404") || lower.contains("403") || lower.contains("feature_disabled")
        }
        TransportError::SocketClosed => true,
    }
}

pub fn run_tunnel_control_once<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_version: &str,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelControlTransport,
{
    let mut socket = match transport.connect_tunnel_control(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_non_fatal_tunnel_control_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    socket.send_message(&build_hello(agent_version))?;
    let mut latest_revision = String::new();
    let mut accepted_rules = Vec::new();
    while let Some(bytes) = socket.read_message()? {
        match parse_server_message(&bytes) {
            Ok(TunnelControlServerMessage::HelloAck { .. }) => {}
            Ok(TunnelControlServerMessage::RuleSync { revision, rules }) => {
                latest_revision = revision;
                accepted_rules = rules;
                socket.send_message(&build_rule_ack(&latest_revision, &accepted_rules, &[]))?;
            }
            Ok(TunnelControlServerMessage::Error { code, message }) => {
                if code == "feature_disabled" {
                    return Ok(());
                }
                return Err(TransportError::RequestFailed(message));
            }
            Err(error) => {
                return Err(TransportError::RequestFailed(error.to_string()));
            }
        }
    }
    let active_rules = accepted_rules.iter().map(|rule| rule.id).collect::<Vec<_>>();
    socket.send_message(&build_heartbeat(&latest_revision, &active_rules))?;
    Ok(())
}
```

- [ ] **Step 4: Run runtime tests**

Run:

```powershell
cargo test --test tunnel_control
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```powershell
git add src/tunnel_control.rs tests/tunnel_control.rs
git commit -m "Add tunnel control agent runtime"
```

---

### Task 8: Agent Tungstenite Tunnel Control Connector And Main Integration

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\tunnel_control.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\main.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\tunnel_control.rs`

- [ ] **Step 1: Add connector request test**

Append to `tests/tunnel_control.rs`:

```rust
#[test]
fn tungstenite_tunnel_control_connector_uses_access_headers_and_url() {
    let line = tunnel_control_startup_line("wss://panel.example.com/api/clients/tunnel?token=secret", true);

    assert_eq!(
        line,
        "tunnel control: enabled url=wss://panel.example.com/api/clients/tunnel?token=redacted"
    );
}
```

- [ ] **Step 2: Run failing connector test**

Run:

```powershell
cargo test --test tunnel_control tungstenite_tunnel_control_connector
```

Expected: FAIL with undefined `tunnel_control_startup_line`.

- [ ] **Step 3: Add Tungstenite connector and startup line**

Append to `src/tunnel_control.rs`:

```rust
use crate::transport::connect_websocket_request;
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[derive(Debug, Default, Clone)]
pub struct TungsteniteTunnelControlTransport {
    custom_dns: String,
}

impl TungsteniteTunnelControlTransport {
    pub fn new_with_custom_dns(custom_dns: &str) -> Self {
        Self { custom_dns: custom_dns.trim().to_string() }
    }
}

impl TunnelControlTransport for TungsteniteTunnelControlTransport {
    type Socket = TungsteniteTunnelControlSocket;

    fn connect_tunnel_control(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        let mut request = url
            .into_client_request()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            request.headers_mut().insert(header_name, header_value);
        }
        let (socket, _response) = connect_websocket_request(request, &self.custom_dns)?;
        Ok(TungsteniteTunnelControlSocket {
            socket,
            read_timeout: Duration::from_millis(500),
        })
    }
}

pub struct TungsteniteTunnelControlSocket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    read_timeout: Duration,
}

impl TunnelControlSocket for TungsteniteTunnelControlSocket {
    fn send_message(&mut self, message: &TunnelControlClientMessage) -> Result<(), TransportError> {
        let payload = serde_json::to_string(message)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        self.socket
            .send(Message::Text(payload.into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.socket
            .get_mut()
            .set_read_timeout(Some(self.read_timeout))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        match self.socket.read() {
            Ok(Message::Text(text)) => Ok(Some(text.to_string().into_bytes())),
            Ok(Message::Binary(bytes)) => Ok(Some(bytes.to_vec())),
            Ok(Message::Close(_)) => Err(TransportError::SocketClosed),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
            {
                Ok(None)
            }
            Err(error) => Err(TransportError::RequestFailed(error.to_string())),
        }
    }
}

pub fn tunnel_control_startup_line(url: &str, enabled: bool) -> String {
    if !enabled {
        return "tunnel control: disabled".to_string();
    }
    let redacted = url
        .split_once("token=")
        .map(|(prefix, _)| format!("{prefix}token=redacted"))
        .unwrap_or_else(|| url.to_string());
    format!("tunnel control: enabled url={redacted}")
}
```

- [ ] **Step 4: Start tunnel control from main**

In `src/main.rs`, update imports:

```rust
use kelicloud_agent_rs::protocol::build_tunnel_control_ws_url;
use kelicloud_agent_rs::tunnel_control::{
    run_tunnel_control_once, tunnel_control_startup_line, TungsteniteTunnelControlTransport,
};
use kelicloud_agent_rs::transport::{access_headers, ReqwestHttpTransport, TungsteniteWebSocketTransport};
```

After `let shared_token = SharedAgentToken::new(config.token.clone());`, add:

```rust
let tunnel_control_url = build_tunnel_control_ws_url(&config.endpoint, &config.token).ok();
if let Some(url) = tunnel_control_url.as_deref() {
    println!("{}", tunnel_control_startup_line(url, config.tunnel_control_enabled));
} else if config.tunnel_control_enabled {
    println!("tunnel control: enabled url=invalid");
}
```

Before the report loop result is built, add:

```rust
if config.tunnel_control_enabled {
    if let Some(url) = tunnel_control_url.clone() {
        let headers = access_headers(&config);
        let agent_version = env!("CARGO_PKG_VERSION").to_string();
        let custom_dns = config.custom_dns.clone();
        if config.once {
            let mut tunnel_transport = TungsteniteTunnelControlTransport::new_with_custom_dns(&custom_dns);
            if let Err(error) = run_tunnel_control_once(&url, &headers, &agent_version, &mut tunnel_transport) {
                eprintln!("tunnel control warning: {error}");
            }
        } else {
            std::thread::spawn(move || {
                let mut tunnel_transport = TungsteniteTunnelControlTransport::new_with_custom_dns(&custom_dns);
                loop {
                    if let Err(error) = run_tunnel_control_once(&url, &headers, &agent_version, &mut tunnel_transport) {
                        eprintln!("tunnel control warning: {error}");
                    }
                    std::thread::sleep(std::time::Duration::from_secs(15));
                }
            });
        }
    }
}
```

This code must not share mutable report-loop state with the tunnel thread.

- [ ] **Step 5: Run agent tests**

Run:

```powershell
cargo test --test tunnel_control
cargo test startup_summary --test runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add src/tunnel_control.rs src/main.rs tests/tunnel_control.rs
git commit -m "Start tunnel control from agent"
```

---

### Task 9: Integration Verification And Publishing

**Files:**
- Modify if needed: `C:\Users\Administrator\Documents\tanzhen\kelicloud\frontend-source.env`

- [ ] **Step 1: Run backend targeted tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./database/tunnel -count=1
go test ./api/client -run TunnelControl -count=1
go test ./api/admin -run Test.*Tunnel -count=1
go test ./config -run TestTunnelFeatureIsVisibleAndDependsOnClients -count=1
```

Expected: PASS. If local `go` is unavailable, push backend and verify the same coverage through GitHub Actions.

- [ ] **Step 2: Run agent tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`:

```powershell
cargo fmt
cargo test tunnel_control
cargo test --test protocol --test config --test runtime
```

Expected: PASS.

- [ ] **Step 3: Run web smoke verification**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-web`:

```powershell
node script\run-unit-tests.mjs
npm run build
```

Expected: PASS. The Web page should need no Phase 2 layout changes.

- [ ] **Step 4: Push agent-rs**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`:

```powershell
git status --short
git push origin main
```

Expected: push succeeds. The repo may already be ahead because Phase 1/Phase 2 docs were committed locally.

- [ ] **Step 5: Push backend and trigger build**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
git status --short
git push origin main
```

Expected: backend push succeeds and GitHub Actions start.

- [ ] **Step 6: Verify GitHub Actions**

Check backend Actions for the pushed commit:

```powershell
$sha = git rev-parse HEAD
$runs = Invoke-RestMethod -Uri "https://api.github.com/repos/keli-123456/kelicloud/actions/runs?head_sha=$sha&per_page=10" -Headers @{ "User-Agent" = "codex"; "Accept" = "application/vnd.github+json" }
$runs.workflow_runs | Select-Object name,status,conclusion,html_url
```

Expected:

- `Build Binaries on Main Push and PR`: completed success.
- `Publish Docker Image on Main`: triggered; if still running, report current state honestly with link.

- [ ] **Step 7: Completion audit**

Confirm these statements from current evidence:

- Backend exposes `/api/clients/tunnel`.
- Agent sends `hello`, receives `rule_sync`, sends `rule_ack`, and heartbeat.
- Rule sync is scoped by client token owner and group.
- Admin status can return `unsupported_agent`, `relay_unavailable`, and `ok`.
- No code opens listeners, dials target services, relays bytes, or implements KTP/TLS frames.

Only after all items are verified, mark the goal complete.

---

## Self-Review

Spec coverage:

- `/api/clients/tunnel` endpoint: Task 4.
- Capability registration state: Tasks 1 and 2.
- Rule version and rule sync: Tasks 2, 4, 6, and 7.
- Heartbeat/status report: Tasks 2, 4, and 7.
- Permission and feature gate: Task 4.
- Agent isolation from report loop: Tasks 7 and 8.
- No KTP/TLS data plane and no listeners: Scope Check, Task 7 tests, Task 9 completion audit.

Type consistency:

- Backend control protocol constant is `keli-tunnel-control.v1`.
- Rule roles are `ingress`, `egress`, and `both`.
- Agent message names use JSON `type` with snake_case variants.
- Backend selected rule JSON keys match agent `SelectedTunnelRule`.

Known implementation caution:

- `database/tunnel/control.go` must import `errors`; it is called out in Task 2.
- `src/main.rs` must clone tunnel-control inputs before the main report loop moves config/token values.
- Tunnel-control startup failures are warnings, not fatal process exits.
