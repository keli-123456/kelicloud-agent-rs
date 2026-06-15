# Tunnel Forwarding Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first tunnel forwarding framework slice: backend schema/API and a dedicated web page for group-based tunnel rules, with no tunnel data traffic yet.

**Architecture:** Phase 1 introduces tunnel rules as user-owned records based on existing client `group` strings. The backend validates ownership, non-empty groups, duplicate listen ports, and status aggregation; the web UI exposes a separate **Tunnel Forwarding** admin page. Agent data-plane work, KTP handshake, relay sessions, and TCP forwarding remain outside this phase.

**Tech Stack:** Go + Gin + GORM + existing kelicloud user feature gates; React + TypeScript + existing admin shell components; existing `node script\run-unit-tests.mjs`, `npm run build`, and targeted Go tests.

---

## Scope Check

The approved spec spans backend, web, and agent data-plane work. This plan intentionally covers only the independently testable first phase:

- Rule models and persistence.
- User feature gate.
- Admin CRUD/status APIs.
- Group catalog based on existing `models.Client.Group`.
- Web API helper.
- Dedicated `/admin/tunnels` page and menu entry.

This phase does not:

- Add `/api/clients/tunnel`.
- Add KTP frame code.
- Open listeners on agents.
- Relay TCP bytes.
- Change existing monitor/report/exec/ping/WebSSH behavior.

## File Structure

Backend repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud`

- Create `database/models/tunnel.go`
  - Owns tunnel constants and GORM models.
- Modify `database/dbcore/dbcore.go`
  - Adds tunnel models to auto migration.
- Create `database/tunnel/tunnel.go`
  - Owns normalization, validation helpers, group catalog, CRUD, conflict checks, and status derivation.
- Create `database/tunnel/tunnel_test.go`
  - Tests persistence, user scoping, non-empty group catalog, empty preserved groups, and conflict detection.
- Create `api/admin/tunnel.go`
  - Owns admin request/response shapes and handlers.
- Create `api/admin/tunnel_test.go`
  - Tests validation and user scoping through handler-level helpers.
- Modify `cmd/server.go`
  - Registers `/api/admin/tunnels` under `RequireUserFeatureMiddleware(config.UserFeatureTunnels)`.
- Modify `config/user_policy.go`
  - Adds `tunnels` as a visible user feature depending on `clients`.
- Modify `api/admin/feature.go`
  - Adds readable denied message for `tunnels`.
- Modify `database/clients/client.go`
  - Adds tunnel cleanup to `deleteClientTx` only if a later direct client status table is introduced. For this phase no client cleanup is required because rules reference groups, not client UUIDs.

Web repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web`

- Create `src/lib/tunnels.ts`
  - Owns request helpers and response normalization.
- Create `src/pages/admin/tunnels.tsx`
  - Dedicated tunnel forwarding page.
- Modify `src/routes.ts`
  - Adds `/admin/tunnels`.
- Modify `src/config/menuConfig.json`
  - Adds Tunnel Forwarding menu item near Servers.
- Modify `src/utils/iconHelper.ts`
  - Uses an existing `Network` or `Workflow` icon; no new icon dependency.
- Modify locale files under `src/i18n/locales/*.json`
  - Adds `tunnels` labels.
- Modify `src/contexts/AccountContext.tsx` and `src/pages/admin/users.tsx`
  - Adds feature string if the context or user access editor keeps a typed list.
- Modify `script/run-unit-tests.mjs`
  - Adds static tests for route/menu/API normalizers.

Agent repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`

- No Rust implementation files are modified in this phase.
- This plan document is the only agent-repo artifact for phase 1.

---

### Task 1: Backend Feature Gate And Models

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\config\user_policy.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\feature.go`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\models\tunnel.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\dbcore\dbcore.go`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud\config\user_test.go`

- [ ] **Step 1: Write failing config tests**

Add these assertions to `config/user_test.go`:

```go
func TestTunnelFeatureIsVisibleAndDependsOnClients(t *testing.T) {
	features := UserAvailableFeatures()
	if !slices.Contains(features, UserFeatureTunnels) {
		t.Fatalf("expected tunnels to be a visible user feature, got %+v", features)
	}

	normalized := NormalizeAllowedFeatures([]string{UserFeatureTunnels})
	if !slices.Contains(normalized, UserFeatureTunnels) {
		t.Fatalf("expected tunnels to stay in normalized features, got %+v", normalized)
	}
	if !slices.Contains(normalized, UserFeatureClients) {
		t.Fatalf("expected tunnels to add clients dependency, got %+v", normalized)
	}
}
```

If `config/user_test.go` does not already import `slices`, add it to the import block:

```go
import "slices"
```

- [ ] **Step 2: Run the failing config test**

Run:

```powershell
go test ./config -run TestTunnelFeatureIsVisibleAndDependsOnClients
```

Expected: FAIL with `undefined: UserFeatureTunnels`.

- [ ] **Step 3: Add the tunnel user feature**

In `config/user_policy.go`, add the constant:

```go
UserFeatureTunnels = "tunnels"
```

Add it to `userFeatureSet`:

```go
UserFeatureTunnels: {},
```

Add it to `userVisibleFeatureSet`:

```go
UserFeatureTunnels: {},
```

Add dependency on servers:

```go
UserFeatureTunnels: {UserFeatureClients},
```

In `api/admin/feature.go`, extend `featureDisplayName`:

```go
case config.UserFeatureTunnels:
	return "Tunnel forwarding"
```

- [ ] **Step 4: Add tunnel models**

Create `database/models/tunnel.go`:

```go
package models

const (
	TunnelProtocolTCP = "tcp"

	TunnelStatusOK                = "ok"
	TunnelStatusPartial           = "partial"
	TunnelStatusDisabled          = "disabled"
	TunnelStatusEmptyIngressGroup = "empty_ingress_group"
	TunnelStatusEmptyEgressGroup  = "empty_egress_group"
	TunnelStatusUnsupportedAgent  = "unsupported_agent"
	TunnelStatusListenFailed      = "listen_failed"
	TunnelStatusRelayUnavailable  = "relay_unavailable"
	TunnelStatusTargetFailed      = "target_failed"
	TunnelStatusAuthFailed        = "auth_failed"
)

type TunnelRule struct {
	ID                    uint      `json:"id,omitempty" gorm:"primaryKey;autoIncrement"`
	UserID                string    `json:"user_id,omitempty" gorm:"type:varchar(36);not null;index:idx_tunnel_user_group"`
	Name                  string    `json:"name" gorm:"type:varchar(100);not null"`
	Enabled               bool      `json:"enabled" gorm:"default:true;index"`
	Protocol              string    `json:"protocol" gorm:"type:varchar(8);not null;default:'tcp';index"`
	IngressGroup          string    `json:"ingress_group" gorm:"type:varchar(100);not null;index:idx_tunnel_user_group"`
	ListenAddress         string    `json:"listen_address" gorm:"type:varchar(100);not null;default:'0.0.0.0'"`
	ListenPort            int       `json:"listen_port" gorm:"not null;index"`
	EgressGroup           string    `json:"egress_group" gorm:"type:varchar(100);not null;index"`
	TargetHost            string    `json:"target_host" gorm:"type:varchar(255);not null"`
	TargetPort            int       `json:"target_port" gorm:"not null"`
	SourceAllowlist       string    `json:"source_allowlist" gorm:"type:varchar(255);not null;default:'0.0.0.0/0'"`
	MaxConcurrentSessions int       `json:"max_concurrent_sessions" gorm:"not null;default:32"`
	Remark                string    `json:"remark" gorm:"type:text"`
	LastRevision           int64     `json:"last_revision" gorm:"not null;default:1"`
	LastError              string    `json:"last_error" gorm:"type:text"`
	CreatedAt             LocalTime `json:"created_at"`
	UpdatedAt             LocalTime `json:"updated_at"`
}
```

- [ ] **Step 5: Add model migration**

In `database/dbcore/dbcore.go`, add `&models.TunnelRule{}` to the general `AutoMigrate` block that already contains `&models.ClientPortForwardRule{}`:

```go
&models.ClientPortForwardRule{},
&models.TunnelRule{},
&models.ClientConditionScriptRule{},
```

- [ ] **Step 6: Run config tests**

Run:

```powershell
go test ./config -run "TestTunnelFeature|TestNormalizeAllowedFeatures"
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```powershell
git add config/user_policy.go api/admin/feature.go database/models/tunnel.go database/dbcore/dbcore.go config/user_test.go
git commit -m "Add tunnel forwarding feature model"
```

---

### Task 2: Backend Tunnel Database Service

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\tunnel.go`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\tunnel_test.go`

- [ ] **Step 1: Write failing database tests**

Create `database/tunnel/tunnel_test.go`:

```go
package tunnel

import (
	"testing"
	"time"

	"github.com/komari-monitor/komari/database/models"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
)

func newTunnelTestDB(t *testing.T) *gorm.DB {
	t.Helper()
	db, err := gorm.Open(sqlite.Open(t.TempDir()+"/tunnel.db"), &gorm.Config{})
	if err != nil {
		t.Fatalf("open test db: %v", err)
	}
	if err := db.AutoMigrate(&models.Client{}, &models.TunnelRule{}); err != nil {
		t.Fatalf("migrate test db: %v", err)
	}
	return db
}

func seedTunnelClient(t *testing.T, db *gorm.DB, userID, uuid, group string) {
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

func TestListGroupsForUserHidesEmptyAndOtherUserGroups(t *testing.T) {
	db := newTunnelTestDB(t)
	seedTunnelClient(t, db, "user-a", "a-1", "edge")
	seedTunnelClient(t, db, "user-a", "a-2", "rdp")
	seedTunnelClient(t, db, "user-b", "b-1", "edge")
	seedTunnelClient(t, db, "user-a", "a-empty-name", "")

	groups, err := listGroupsForUserWithDB(db, "user-a")
	if err != nil {
		t.Fatalf("list groups: %v", err)
	}
	if len(groups) != 2 {
		t.Fatalf("expected two groups, got %+v", groups)
	}
	if groups[0].Name != "edge" || groups[0].ClientCount != 1 {
		t.Fatalf("unexpected first group: %+v", groups[0])
	}
	if groups[1].Name != "rdp" || groups[1].ClientCount != 1 {
		t.Fatalf("unexpected second group: %+v", groups[1])
	}
}

func TestSaveRuleForUserValidatesNonEmptyGroupsAndConflicts(t *testing.T) {
	db := newTunnelTestDB(t)
	seedTunnelClient(t, db, "user-a", "a-1", "edge")
	seedTunnelClient(t, db, "user-a", "a-2", "rdp")

	rule := models.TunnelRule{
		Name:            "RDP",
		Enabled:         true,
		Protocol:        models.TunnelProtocolTCP,
		IngressGroup:    "edge",
		ListenAddress:   "0.0.0.0",
		ListenPort:      10088,
		EgressGroup:     "rdp",
		TargetHost:      "127.0.0.1",
		TargetPort:      3389,
		SourceAllowlist: "0.0.0.0/0",
	}

	saved, err := saveRuleForUserWithDB(db, "user-a", &rule)
	if err != nil {
		t.Fatalf("save rule: %v", err)
	}
	status, err := BuildRuleStatusForUser(db, "user-a", saved)
	if err != nil {
		t.Fatalf("build status: %v", err)
	}
	if saved.ID == 0 || status != models.TunnelStatusOK {
		t.Fatalf("expected saved ok rule, got %+v", saved)
	}

	_, err = saveRuleForUserWithDB(db, "user-a", &models.TunnelRule{
		Name:          "Conflict",
		Enabled:       true,
		Protocol:      models.TunnelProtocolTCP,
		IngressGroup:  "edge",
		ListenAddress: "0.0.0.0",
		ListenPort:    10088,
		EgressGroup:   "rdp",
		TargetHost:    "127.0.0.1",
		TargetPort:    3389,
	})
	if err == nil {
		t.Fatal("expected duplicate listen port conflict")
	}
}

func TestExistingRuleCanKeepNowEmptyGroup(t *testing.T) {
	db := newTunnelTestDB(t)
	seedTunnelClient(t, db, "user-a", "a-1", "edge")
	seedTunnelClient(t, db, "user-a", "a-2", "rdp")

	saved, err := saveRuleForUserWithDB(db, "user-a", &models.TunnelRule{
		Name:          "RDP",
		Enabled:       true,
		Protocol:      models.TunnelProtocolTCP,
		IngressGroup:  "edge",
		ListenAddress: "0.0.0.0",
		ListenPort:    10088,
		EgressGroup:   "rdp",
		TargetHost:    "127.0.0.1",
		TargetPort:    3389,
	})
	if err != nil {
		t.Fatalf("save rule: %v", err)
	}
	if err := db.Model(&models.Client{}).Where("uuid = ?", "a-2").Update("group", "").Error; err != nil {
		t.Fatalf("clear group: %v", err)
	}
	saved.Remark = "kept"
	updated, err := saveRuleForUserWithDB(db, "user-a", &saved)
	if err != nil {
		t.Fatalf("expected update to keep existing empty egress group: %v", err)
	}
	status, err := BuildRuleStatusForUser(db, "user-a", updated)
	if err != nil {
		t.Fatalf("build status: %v", err)
	}
	if status != models.TunnelStatusEmptyEgressGroup {
		t.Fatalf("expected empty egress status, got %s", status)
	}
}
```

- [ ] **Step 2: Run the failing database tests**

Run:

```powershell
go test ./database/tunnel
```

Expected: FAIL because `database/tunnel` and `BuildRuleStatusForUser` do not exist.

- [ ] **Step 3: Add database service implementation**

Create `database/tunnel/tunnel.go`:

```go
package tunnel

import (
	"errors"
	"fmt"
	"net"
	"sort"
	"strings"

	"github.com/komari-monitor/komari/database/dbcore"
	"github.com/komari-monitor/komari/database/models"
	"gorm.io/gorm"
)

type GroupSummary struct {
	Name        string `json:"name"`
	ClientCount int    `json:"client_count"`
}

func normalizeUserID(userUUID string) (string, error) {
	userUUID = strings.TrimSpace(userUUID)
	if userUUID == "" {
		return "", errors.New("user id is required")
	}
	return userUUID, nil
}

func normalizeGroup(value string) string {
	if len([]rune(value)) > 100 {
		value = string([]rune(value)[:100])
	}
	return strings.Join(strings.Fields(strings.TrimSpace(value)), " ")
}

func normalizeProtocol(value string) string {
	if strings.EqualFold(strings.TrimSpace(value), models.TunnelProtocolTCP) {
		return models.TunnelProtocolTCP
	}
	return models.TunnelProtocolTCP
}

func normalizeListenAddress(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return "0.0.0.0"
	}
	return value
}

func normalizeSourceAllowlist(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return "0.0.0.0/0"
	}
	return value
}

func validatePort(label string, port int) error {
	if port < 1 || port > 65535 {
		return fmt.Errorf("%s must be between 1 and 65535", label)
	}
	return nil
}

func validateHost(value string) error {
	value = strings.TrimSpace(value)
	if value == "" {
		return errors.New("target host is required")
	}
	if len(value) > 255 {
		return errors.New("target host is too long")
	}
	if ip := net.ParseIP(value); ip != nil {
		return nil
	}
	if strings.Contains(value, "..") {
		return errors.New("target host is invalid")
	}
	for _, part := range strings.Split(value, ".") {
		if part == "" || len(part) > 63 {
			return errors.New("target host is invalid")
		}
	}
	return nil
}

func groupClientCountWithDB(db *gorm.DB, userUUID, group string) (int64, error) {
	var count int64
	err := db.Model(&models.Client{}).
		Where("user_id = ? AND `group` = ?", userUUID, group).
		Count(&count).Error
	return count, err
}

func listGroupsForUserWithDB(db *gorm.DB, userUUID string) ([]GroupSummary, error) {
	normalizedUserID, err := normalizeUserID(userUUID)
	if err != nil {
		return nil, err
	}

	type row struct {
		Name  string
		Total int64
	}
	var rows []row
	if err := db.Model(&models.Client{}).
		Select("`group` AS name, COUNT(*) AS total").
		Where("user_id = ? AND `group` <> ''", normalizedUserID).
		Group("`group`").
		Order("`group` ASC").
		Scan(&rows).Error; err != nil {
		return nil, err
	}

	groups := make([]GroupSummary, 0, len(rows))
	for _, row := range rows {
		groups = append(groups, GroupSummary{Name: row.Name, ClientCount: int(row.Total)})
	}
	return groups, nil
}

func ListGroupsForUser(userUUID string) ([]GroupSummary, error) {
	return listGroupsForUserWithDB(dbcore.GetDBInstance(), userUUID)
}

func normalizeRule(rule *models.TunnelRule, userUUID string) (models.TunnelRule, error) {
	if rule == nil {
		return models.TunnelRule{}, errors.New("rule is required")
	}
	normalizedUserID, err := normalizeUserID(userUUID)
	if err != nil {
		return models.TunnelRule{}, err
	}

	next := *rule
	next.UserID = normalizedUserID
	next.Name = strings.TrimSpace(next.Name)
	if next.Name == "" {
		next.Name = "TCP Tunnel"
	}
	next.Protocol = normalizeProtocol(next.Protocol)
	next.IngressGroup = normalizeGroup(next.IngressGroup)
	next.EgressGroup = normalizeGroup(next.EgressGroup)
	next.ListenAddress = normalizeListenAddress(next.ListenAddress)
	next.TargetHost = strings.TrimSpace(next.TargetHost)
	next.SourceAllowlist = normalizeSourceAllowlist(next.SourceAllowlist)
	next.Remark = strings.TrimSpace(next.Remark)
	next.LastError = strings.TrimSpace(next.LastError)
	if next.MaxConcurrentSessions <= 0 {
		next.MaxConcurrentSessions = 32
	}

	if next.IngressGroup == "" {
		return models.TunnelRule{}, errors.New("ingress group is required")
	}
	if next.EgressGroup == "" {
		return models.TunnelRule{}, errors.New("egress group is required")
	}
	if err := validatePort("listen port", next.ListenPort); err != nil {
		return models.TunnelRule{}, err
	}
	if err := validatePort("target port", next.TargetPort); err != nil {
		return models.TunnelRule{}, err
	}
	if err := validateHost(next.TargetHost); err != nil {
		return models.TunnelRule{}, err
	}
	return next, nil
}

func validateGroupAvailabilityForSave(db *gorm.DB, existing *models.TunnelRule, next models.TunnelRule) error {
	ingressCount, err := groupClientCountWithDB(db, next.UserID, next.IngressGroup)
	if err != nil {
		return err
	}
	if ingressCount == 0 && (existing == nil || existing.IngressGroup != next.IngressGroup) {
		return fmt.Errorf("ingress group %q has no machines", next.IngressGroup)
	}

	egressCount, err := groupClientCountWithDB(db, next.UserID, next.EgressGroup)
	if err != nil {
		return err
	}
	if egressCount == 0 && (existing == nil || existing.EgressGroup != next.EgressGroup) {
		return fmt.Errorf("egress group %q has no machines", next.EgressGroup)
	}
	return nil
}

func findEnabledListenConflictWithDB(db *gorm.DB, rule models.TunnelRule) (models.TunnelRule, error) {
	query := db.Where(
		"user_id = ? AND enabled = ? AND protocol = ? AND ingress_group = ? AND listen_address = ? AND listen_port = ?",
		rule.UserID,
		true,
		rule.Protocol,
		rule.IngressGroup,
		rule.ListenAddress,
		rule.ListenPort,
	)
	if rule.ID > 0 {
		query = query.Where("id <> ?", rule.ID)
	}
	var conflict models.TunnelRule
	err := query.First(&conflict).Error
	return conflict, err
}

func saveRuleForUserWithDB(db *gorm.DB, userUUID string, rule *models.TunnelRule) (models.TunnelRule, error) {
	next, err := normalizeRule(rule, userUUID)
	if err != nil {
		return models.TunnelRule{}, err
	}

	var existing *models.TunnelRule
	if next.ID > 0 {
		var loaded models.TunnelRule
		if err := db.Where("user_id = ? AND id = ?", next.UserID, next.ID).First(&loaded).Error; err != nil {
			return models.TunnelRule{}, err
		}
		existing = &loaded
		next.LastRevision = loaded.LastRevision + 1
		if next.LastRevision <= 1 {
			next.LastRevision = 2
		}
	} else {
		next.LastRevision = 1
	}

	if err := validateGroupAvailabilityForSave(db, existing, next); err != nil {
		return models.TunnelRule{}, err
	}
	if next.Enabled {
		if conflict, err := findEnabledListenConflictWithDB(db, next); err == nil {
			return models.TunnelRule{}, fmt.Errorf("listen port %d is already used by tunnel %q", conflict.ListenPort, conflict.Name)
		} else if !errors.Is(err, gorm.ErrRecordNotFound) {
			return models.TunnelRule{}, err
		}
	}

	if next.ID == 0 {
		if err := db.Create(&next).Error; err != nil {
			return models.TunnelRule{}, err
		}
		return next, nil
	}
	if err := db.Save(&next).Error; err != nil {
		return models.TunnelRule{}, err
	}
	return next, nil
}

func SaveRuleForUser(userUUID string, rule *models.TunnelRule) (models.TunnelRule, error) {
	return saveRuleForUserWithDB(dbcore.GetDBInstance(), userUUID, rule)
}

func ListRulesForUser(userUUID string) ([]models.TunnelRule, error) {
	normalizedUserID, err := normalizeUserID(userUUID)
	if err != nil {
		return nil, err
	}
	var rules []models.TunnelRule
	err = dbcore.GetDBInstance().
		Where("user_id = ?", normalizedUserID).
		Order("id DESC").
		Find(&rules).Error
	return rules, err
}

func DeleteRuleForUser(userUUID string, id uint) error {
	normalizedUserID, err := normalizeUserID(userUUID)
	if err != nil {
		return err
	}
	result := dbcore.GetDBInstance().
		Where("user_id = ? AND id = ?", normalizedUserID, id).
		Delete(&models.TunnelRule{})
	if result.Error != nil {
		return result.Error
	}
	if result.RowsAffected == 0 {
		return gorm.ErrRecordNotFound
	}
	return nil
}

func BuildRuleStatusForUser(db *gorm.DB, userUUID string, rule models.TunnelRule) (string, error) {
	if !rule.Enabled {
		return models.TunnelStatusDisabled, nil
	}
	ingressCount, err := groupClientCountWithDB(db, userUUID, rule.IngressGroup)
	if err != nil {
		return "", err
	}
	if ingressCount == 0 {
		return models.TunnelStatusEmptyIngressGroup, nil
	}
	egressCount, err := groupClientCountWithDB(db, userUUID, rule.EgressGroup)
	if err != nil {
		return "", err
	}
	if egressCount == 0 {
		return models.TunnelStatusEmptyEgressGroup, nil
	}
	if strings.TrimSpace(rule.LastError) != "" {
		return models.TunnelStatusPartial, nil
	}
	return models.TunnelStatusOK, nil
}

func SortRulesByIDDesc(rules []models.TunnelRule) {
	sort.SliceStable(rules, func(i, j int) bool {
		return rules[i].ID > rules[j].ID
	})
}
```

- [ ] **Step 4: Run database tests**

Run:

```powershell
go test ./database/tunnel
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```powershell
git add database/tunnel/tunnel.go database/tunnel/tunnel_test.go database/models/tunnel.go
git commit -m "Add tunnel rule database service"
```

---

### Task 3: Backend Admin API And Routes

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\tunnel.go`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\tunnel_test.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\cmd\server.go`

- [ ] **Step 1: Write failing handler helper tests**

Create `api/admin/tunnel_test.go`:

```go
package admin

import (
	"testing"

	"github.com/komari-monitor/komari/database/models"
)

func TestNormalizeTunnelRuleRequestDefaultsAndValidation(t *testing.T) {
	req := tunnelRuleRequest{
		Name:          " RDP ",
		Enabled:       boolPtr(true),
		IngressGroup:  " edge ",
		ListenAddress: "",
		ListenPort:    10088,
		EgressGroup:   " rdp ",
		TargetHost:    "127.0.0.1",
		TargetPort:    3389,
	}

	rule, err := normalizeTunnelRuleRequest(req)
	if err != nil {
		t.Fatalf("normalize request: %v", err)
	}
	if rule.Name != "RDP" || rule.Protocol != models.TunnelProtocolTCP {
		t.Fatalf("unexpected normalized rule: %+v", rule)
	}
	if rule.ListenAddress != "0.0.0.0" || rule.SourceAllowlist != "0.0.0.0/0" {
		t.Fatalf("expected default listen and allowlist, got %+v", rule)
	}
	if !rule.Enabled {
		t.Fatal("expected enabled default true from request pointer")
	}
}

func TestBuildTunnelRuleViewUsesDerivedStatus(t *testing.T) {
	rule := models.TunnelRule{
		ID:           7,
		Name:         "RDP",
		Enabled:      true,
		Protocol:     models.TunnelProtocolTCP,
		IngressGroup: "edge",
		ListenPort:   10088,
		EgressGroup:  "rdp",
		TargetHost:   "127.0.0.1",
		TargetPort:   3389,
	}
	view := buildTunnelRuleView(rule, models.TunnelStatusEmptyEgressGroup)
	if view.ID != 7 || view.Status != models.TunnelStatusEmptyEgressGroup {
		t.Fatalf("unexpected view: %+v", view)
	}
}

func boolPtr(value bool) *bool {
	return &value
}
```

- [ ] **Step 2: Run the failing handler tests**

Run:

```powershell
go test ./api/admin -run TestNormalizeTunnelRuleRequest
```

Expected: FAIL because tunnel helper types do not exist.

- [ ] **Step 3: Add admin tunnel handlers**

Create `api/admin/tunnel.go`:

```go
package admin

import (
	"errors"
	"net/http"
	"strconv"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/komari-monitor/komari/api"
	tunneldb "github.com/komari-monitor/komari/database/tunnel"
	"github.com/komari-monitor/komari/database/dbcore"
	"github.com/komari-monitor/komari/database/models"
	"gorm.io/gorm"
)

type tunnelRuleRequest struct {
	ID                    uint   `json:"id"`
	Name                  string `json:"name"`
	Enabled               *bool  `json:"enabled"`
	Protocol              string `json:"protocol"`
	IngressGroup          string `json:"ingress_group"`
	ListenAddress         string `json:"listen_address"`
	ListenPort            int    `json:"listen_port"`
	EgressGroup           string `json:"egress_group"`
	TargetHost            string `json:"target_host"`
	TargetPort            int    `json:"target_port"`
	SourceAllowlist       string `json:"source_allowlist"`
	MaxConcurrentSessions int    `json:"max_concurrent_sessions"`
	Remark                string `json:"remark"`
}

type tunnelRuleView struct {
	ID                    uint              `json:"id"`
	Name                  string            `json:"name"`
	Enabled               bool              `json:"enabled"`
	Protocol              string            `json:"protocol"`
	IngressGroup          string            `json:"ingress_group"`
	ListenAddress         string            `json:"listen_address"`
	ListenPort            int               `json:"listen_port"`
	EgressGroup           string            `json:"egress_group"`
	TargetHost            string            `json:"target_host"`
	TargetPort            int               `json:"target_port"`
	SourceAllowlist       string            `json:"source_allowlist"`
	MaxConcurrentSessions int               `json:"max_concurrent_sessions"`
	Remark                string            `json:"remark"`
	Status                string            `json:"status"`
	LastRevision          int64             `json:"last_revision"`
	LastError             string            `json:"last_error"`
	CreatedAt             models.LocalTime  `json:"created_at"`
	UpdatedAt             models.LocalTime  `json:"updated_at"`
}

func normalizeTunnelRuleRequest(req tunnelRuleRequest) (models.TunnelRule, error) {
	enabled := true
	if req.Enabled != nil {
		enabled = *req.Enabled
	}
	name := strings.TrimSpace(req.Name)
	if name == "" {
		name = "TCP Tunnel"
	}
	protocol := strings.ToLower(strings.TrimSpace(req.Protocol))
	if protocol == "" {
		protocol = models.TunnelProtocolTCP
	}
	listenAddress := strings.TrimSpace(req.ListenAddress)
	if listenAddress == "" {
		listenAddress = "0.0.0.0"
	}
	sourceAllowlist := strings.TrimSpace(req.SourceAllowlist)
	if sourceAllowlist == "" {
		sourceAllowlist = "0.0.0.0/0"
	}
	maxSessions := req.MaxConcurrentSessions
	if maxSessions <= 0 {
		maxSessions = 32
	}
	return models.TunnelRule{
		ID:                    req.ID,
		Name:                  name,
		Enabled:               enabled,
		Protocol:              protocol,
		IngressGroup:          strings.TrimSpace(req.IngressGroup),
		ListenAddress:         listenAddress,
		ListenPort:            req.ListenPort,
		EgressGroup:           strings.TrimSpace(req.EgressGroup),
		TargetHost:            strings.TrimSpace(req.TargetHost),
		TargetPort:            req.TargetPort,
		SourceAllowlist:       sourceAllowlist,
		MaxConcurrentSessions: maxSessions,
		Remark:                strings.TrimSpace(req.Remark),
	}, nil
}

func buildTunnelRuleView(rule models.TunnelRule, status string) tunnelRuleView {
	return tunnelRuleView{
		ID:                    rule.ID,
		Name:                  rule.Name,
		Enabled:               rule.Enabled,
		Protocol:              rule.Protocol,
		IngressGroup:          rule.IngressGroup,
		ListenAddress:         rule.ListenAddress,
		ListenPort:            rule.ListenPort,
		EgressGroup:           rule.EgressGroup,
		TargetHost:            rule.TargetHost,
		TargetPort:            rule.TargetPort,
		SourceAllowlist:       rule.SourceAllowlist,
		MaxConcurrentSessions: rule.MaxConcurrentSessions,
		Remark:                rule.Remark,
		Status:                status,
		LastRevision:          rule.LastRevision,
		LastError:             rule.LastError,
		CreatedAt:             rule.CreatedAt,
		UpdatedAt:             rule.UpdatedAt,
	}
}

func buildTunnelRuleViews(userUUID string, rules []models.TunnelRule) ([]tunnelRuleView, error) {
	views := make([]tunnelRuleView, 0, len(rules))
	db := dbcore.GetDBInstance()
	for _, rule := range rules {
		status, err := tunneldb.BuildRuleStatusForUser(db, userUUID, rule)
		if err != nil {
			return nil, err
		}
		views = append(views, buildTunnelRuleView(rule, status))
	}
	return views, nil
}

func parseTunnelRuleID(c *gin.Context) (uint, bool) {
	rawID := strings.TrimSpace(c.Param("id"))
	parsed, err := strconv.ParseUint(rawID, 10, 32)
	if err != nil || parsed == 0 {
		api.RespondError(c, http.StatusBadRequest, "隧道规则 ID 无效")
		return 0, false
	}
	return uint(parsed), true
}

func GetTunnelRules(c *gin.Context) {
	scope := CurrentOwnerScope(c)
	if !scope.Valid() {
		api.RespondError(c, http.StatusUnauthorized, "用户无效")
		return
	}
	rules, err := tunneldb.ListRulesForUser(scope.UserUUID)
	if err != nil {
		api.RespondError(c, http.StatusInternalServerError, "加载隧道规则失败: "+err.Error())
		return
	}
	groups, err := tunneldb.ListGroupsForUser(scope.UserUUID)
	if err != nil {
		api.RespondError(c, http.StatusInternalServerError, "加载分组失败: "+err.Error())
		return
	}
	views, err := buildTunnelRuleViews(scope.UserUUID, rules)
	if err != nil {
		api.RespondError(c, http.StatusInternalServerError, "计算隧道状态失败: "+err.Error())
		return
	}
	api.RespondSuccess(c, gin.H{"rules": views, "groups": groups})
}

func SaveTunnelRule(c *gin.Context) {
	scope := CurrentOwnerScope(c)
	if !scope.Valid() {
		api.RespondError(c, http.StatusUnauthorized, "用户无效")
		return
	}
	var req tunnelRuleRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		api.RespondError(c, http.StatusBadRequest, "请求内容无效: "+err.Error())
		return
	}
	rule, err := normalizeTunnelRuleRequest(req)
	if err != nil {
		api.RespondError(c, http.StatusBadRequest, err.Error())
		return
	}
	saved, err := tunneldb.SaveRuleForUser(scope.UserUUID, &rule)
	if err != nil {
		api.RespondError(c, http.StatusBadRequest, err.Error())
		return
	}
	status, err := tunneldb.BuildRuleStatusForUser(dbcore.GetDBInstance(), scope.UserUUID, saved)
	if err != nil {
		api.RespondError(c, http.StatusInternalServerError, "计算隧道状态失败: "+err.Error())
		return
	}
	api.AuditLogForCurrentUser(c, scope.UserUUID, "save tunnel:"+strconv.FormatUint(uint64(saved.ID), 10), "info")
	api.RespondSuccess(c, buildTunnelRuleView(saved, status))
}

func DeleteTunnelRule(c *gin.Context) {
	scope := CurrentOwnerScope(c)
	if !scope.Valid() {
		api.RespondError(c, http.StatusUnauthorized, "用户无效")
		return
	}
	id, ok := parseTunnelRuleID(c)
	if !ok {
		return
	}
	if err := tunneldb.DeleteRuleForUser(scope.UserUUID, id); err != nil {
		if errors.Is(err, gorm.ErrRecordNotFound) {
			api.RespondError(c, http.StatusNotFound, "隧道规则不存在")
			return
		}
		api.RespondError(c, http.StatusInternalServerError, "删除隧道规则失败: "+err.Error())
		return
	}
	api.AuditLogForCurrentUser(c, scope.UserUUID, "delete tunnel:"+strconv.FormatUint(uint64(id), 10), "warn")
	api.RespondSuccess(c, nil)
}
```

- [ ] **Step 4: Register routes**

In `cmd/server.go`, inside the admin group, add:

```go
tunnelGroup := adminAuthrized.Group("/tunnels", admin.RequireUserFeatureMiddleware(config.UserFeatureTunnels))
{
	tunnelGroup.GET("", admin.GetTunnelRules)
	tunnelGroup.POST("", admin.SaveTunnelRule)
	tunnelGroup.POST("/:id/remove", admin.DeleteTunnelRule)
}
```

Place it near the existing `clientGroup` routes so operations pages stay together.

- [ ] **Step 5: Run backend API tests**

Run:

```powershell
go test ./api/admin -run Test.*Tunnel
go test ./database/tunnel
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```powershell
git add api/admin/tunnel.go api/admin/tunnel_test.go cmd/server.go
git commit -m "Add tunnel forwarding admin API"
```

---

### Task 4: Web Tunnel API Helper

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\lib\tunnels.ts`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\script\run-unit-tests.mjs`

- [ ] **Step 1: Write failing web unit tests**

In `script/run-unit-tests.mjs`, load the module:

```js
const tunnels = loadTsModule("src/lib/tunnels.ts");
```

Add this test:

```js
test("tunnel helpers normalize rule lists and group catalogs", () => {
  const data = tunnels.normalizeTunnelListPayload({
    rules: [{
      id: 7,
      name: "RDP",
      enabled: true,
      protocol: "tcp",
      ingress_group: "edge",
      listen_address: "0.0.0.0",
      listen_port: 10088,
      egress_group: "rdp",
      target_host: "127.0.0.1",
      target_port: 3389,
      source_allowlist: "0.0.0.0/0",
      max_concurrent_sessions: 32,
      status: "ok",
    }],
    groups: [{ name: "edge", client_count: 2 }],
  });

  assert.equal(data.rules.length, 1);
  assert.equal(data.rules[0].status, "ok");
  assert.equal(data.groups[0].name, "edge");
  assert.equal(data.groups[0].client_count, 2);
});
```

- [ ] **Step 2: Run the failing web test**

Run:

```powershell
node script\run-unit-tests.mjs
```

Expected: FAIL because `src/lib/tunnels.ts` does not exist.

- [ ] **Step 3: Add tunnel API helper**

Create `src/lib/tunnels.ts`:

```ts
import { formatApiErrorMessage } from "@/lib/apiErrorMessage";

export class TunnelApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(formatApiErrorMessage(message, { status }));
    this.name = "TunnelApiError";
    this.status = status;
  }
}

type ApiEnvelope<T> = {
  status?: string;
  message?: string;
  data?: T;
};

export type TunnelRuleStatus =
  | "ok"
  | "partial"
  | "disabled"
  | "empty_ingress_group"
  | "empty_egress_group"
  | "unsupported_agent"
  | "listen_failed"
  | "relay_unavailable"
  | "target_failed"
  | "auth_failed";

export type TunnelGroupSummary = {
  name: string;
  client_count: number;
};

export type TunnelRule = {
  id: number;
  name: string;
  enabled: boolean;
  protocol: "tcp";
  ingress_group: string;
  listen_address: string;
  listen_port: number;
  egress_group: string;
  target_host: string;
  target_port: number;
  source_allowlist: string;
  max_concurrent_sessions: number;
  remark: string;
  status: TunnelRuleStatus;
  last_revision: number;
  last_error: string;
  created_at: string;
  updated_at: string;
};

export type TunnelRuleInput = {
  id?: number;
  name: string;
  enabled: boolean;
  ingress_group: string;
  listen_address: string;
  listen_port: number;
  egress_group: string;
  target_host: string;
  target_port: number;
  source_allowlist: string;
  max_concurrent_sessions: number;
  remark: string;
};

export type TunnelListPayload = {
  rules: TunnelRule[];
  groups: TunnelGroupSummary[];
};

const TUNNEL_REQUEST_TIMEOUT_MS = 30_000;

function normalizeString(value: unknown) {
  return typeof value === "string" ? value : "";
}

function normalizeNumber(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function normalizeBoolean(value: unknown) {
  return typeof value === "boolean" ? value : Boolean(value);
}

function normalizeStatus(value: unknown): TunnelRuleStatus {
  const status = normalizeString(value) as TunnelRuleStatus;
  return status || "disabled";
}

function normalizeTunnelRule(value: unknown): TunnelRule | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  return {
    id: normalizeNumber(raw.id),
    name: normalizeString(raw.name),
    enabled: normalizeBoolean(raw.enabled),
    protocol: "tcp",
    ingress_group: normalizeString(raw.ingress_group),
    listen_address: normalizeString(raw.listen_address) || "0.0.0.0",
    listen_port: normalizeNumber(raw.listen_port),
    egress_group: normalizeString(raw.egress_group),
    target_host: normalizeString(raw.target_host),
    target_port: normalizeNumber(raw.target_port),
    source_allowlist: normalizeString(raw.source_allowlist) || "0.0.0.0/0",
    max_concurrent_sessions: normalizeNumber(raw.max_concurrent_sessions) || 32,
    remark: normalizeString(raw.remark),
    status: normalizeStatus(raw.status),
    last_revision: normalizeNumber(raw.last_revision),
    last_error: normalizeString(raw.last_error),
    created_at: normalizeString(raw.created_at),
    updated_at: normalizeString(raw.updated_at),
  };
}

function normalizeTunnelGroup(value: unknown): TunnelGroupSummary | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const name = normalizeString(raw.name).trim();
  if (!name) return null;
  return {
    name,
    client_count: normalizeNumber(raw.client_count),
  };
}

export function normalizeTunnelListPayload(value: unknown): TunnelListPayload {
  const raw = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const rules = Array.isArray(raw.rules)
    ? raw.rules.flatMap((item) => {
        const rule = normalizeTunnelRule(item);
        return rule ? [rule] : [];
      })
    : [];
  const groups = Array.isArray(raw.groups)
    ? raw.groups.flatMap((item) => {
        const group = normalizeTunnelGroup(item);
        return group ? [group] : [];
      })
    : [];
  return { rules, groups };
}

async function requestTunnels<T>(path: string, init?: RequestInit): Promise<T> {
  const method = (init?.method || "GET").toUpperCase();
  const requestUrl = method === "GET"
    ? `${path}${path.includes("?") ? "&" : "?"}__ts=${Date.now()}`
    : path;
  const controller = new AbortController();
  const timeoutID = setTimeout(() => controller.abort(), TUNNEL_REQUEST_TIMEOUT_MS);
  try {
    const response = await fetch(requestUrl, {
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Cache-Control": "no-cache, no-store, max-age=0",
        Pragma: "no-cache",
        "X-Requested-With": "XMLHttpRequest",
        ...(init?.headers || {}),
      },
      ...init,
      signal: controller.signal,
    });
    const text = await response.text();
    const payload = text.trim() ? JSON.parse(text) as ApiEnvelope<T> : null;
    if (!response.ok || payload?.status === "error") {
      throw new TunnelApiError(payload?.message || `HTTP ${response.status}`, response.status);
    }
    return payload?.data as T;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new TunnelApiError(`请求隧道接口超时: ${path}`, 408);
    }
    throw error;
  } finally {
    clearTimeout(timeoutID);
  }
}

export async function getTunnelRules(): Promise<TunnelListPayload> {
  const data = await requestTunnels<unknown>("/api/admin/tunnels");
  return normalizeTunnelListPayload(data);
}

export async function saveTunnelRule(input: TunnelRuleInput): Promise<TunnelRule> {
  const data = await requestTunnels<unknown>("/api/admin/tunnels", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ...input, protocol: "tcp" }),
  });
  const rule = normalizeTunnelRule(data);
  if (!rule) throw new TunnelApiError("隧道接口返回了无效规则", 500);
  return rule;
}

export async function deleteTunnelRule(ruleID: number): Promise<void> {
  await requestTunnels<unknown>(`/api/admin/tunnels/${ruleID}/remove`, {
    method: "POST",
  });
}
```

- [ ] **Step 4: Run web unit tests**

Run:

```powershell
node script\run-unit-tests.mjs
```

Expected: PASS including the new tunnel normalizer test.

- [ ] **Step 5: Commit**

Run:

```powershell
git add src/lib/tunnels.ts script/run-unit-tests.mjs
git commit -m "Add tunnel forwarding web API helper"
```

---

### Task 5: Web Tunnel Forwarding Page

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\pages\admin\tunnels.tsx`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\routes.ts`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\config\menuConfig.json`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\i18n\locales\zh_CN.json`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\i18n\locales\zh_TW.json`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\i18n\locales\en.json`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\i18n\locales\ja_JP.json`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\i18n\locales\id_ID.json`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\script\run-unit-tests.mjs`

- [ ] **Step 1: Write failing static route/menu test**

In `script/run-unit-tests.mjs`, add:

```js
test("tunnel forwarding page is routed and shown in admin menu", () => {
  const routesSource = fs.readFileSync(path.resolve(root, "src/routes.ts"), "utf8");
  const menuSource = fs.readFileSync(path.resolve(root, "src/config/menuConfig.json"), "utf8");

  assert.match(routesSource, /path:\s*"tunnels"/);
  assert.match(routesSource, /pages\/admin\/tunnels/);
  assert.match(menuSource, /"path":\s*"\/admin\/tunnels"/);
  assert.match(menuSource, /"labelKey":\s*"tunnels\.title"/);
});
```

- [ ] **Step 2: Run failing web unit test**

Run:

```powershell
node script\run-unit-tests.mjs
```

Expected: FAIL because the route and menu entry do not exist.

- [ ] **Step 3: Add the page component**

Create `src/pages/admin/tunnels.tsx`:

```tsx
import React from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import {
  AdminPanel,
  AdminPanelBody,
  AdminPanelHeader,
  AdminPageShell,
  AdminTableSkeleton,
} from "@/components/admin/AdminPageShell";
import {
  AdminDataTable,
  AdminDataTableBody,
  AdminDataTableCell,
  AdminDataTableHead,
  AdminDataTableHeader,
  AdminDataTableRow,
} from "@/components/admin/AdminDataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  deleteTunnelRule,
  getTunnelRules,
  saveTunnelRule,
  type TunnelGroupSummary,
  type TunnelRule,
  type TunnelRuleInput,
} from "@/lib/tunnels";
import { getReadableErrorMessage } from "@/lib/apiErrorMessage";

type TunnelFormState = {
  id?: number;
  name: string;
  enabled: boolean;
  ingress_group: string;
  listen_address: string;
  listen_port: string;
  egress_group: string;
  target_host: string;
  target_port: string;
  source_allowlist: string;
  max_concurrent_sessions: string;
  remark: string;
};

const EMPTY_FORM: TunnelFormState = {
  name: "RDP",
  enabled: true,
  ingress_group: "",
  listen_address: "0.0.0.0",
  listen_port: "10088",
  egress_group: "",
  target_host: "127.0.0.1",
  target_port: "3389",
  source_allowlist: "0.0.0.0/0",
  max_concurrent_sessions: "32",
  remark: "",
};

function ruleToForm(rule: TunnelRule): TunnelFormState {
  return {
    id: rule.id,
    name: rule.name,
    enabled: rule.enabled,
    ingress_group: rule.ingress_group,
    listen_address: rule.listen_address,
    listen_port: String(rule.listen_port || 10088),
    egress_group: rule.egress_group,
    target_host: rule.target_host,
    target_port: String(rule.target_port || 3389),
    source_allowlist: rule.source_allowlist || "0.0.0.0/0",
    max_concurrent_sessions: String(rule.max_concurrent_sessions || 32),
    remark: rule.remark || "",
  };
}

function parsePort(label: string, value: string) {
  const port = Number.parseInt(value, 10);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(`${label} 必须在 1 到 65535 之间`);
  }
  return port;
}

function buildRuleInput(form: TunnelFormState): TunnelRuleInput {
  return {
    id: form.id,
    name: form.name.trim() || "TCP Tunnel",
    enabled: form.enabled,
    ingress_group: form.ingress_group.trim(),
    listen_address: form.listen_address.trim() || "0.0.0.0",
    listen_port: parsePort("监听端口", form.listen_port),
    egress_group: form.egress_group.trim(),
    target_host: form.target_host.trim(),
    target_port: parsePort("目标端口", form.target_port),
    source_allowlist: form.source_allowlist.trim() || "0.0.0.0/0",
    max_concurrent_sessions:
      Number.parseInt(form.max_concurrent_sessions || "32", 10) || 32,
    remark: form.remark.trim(),
  };
}

function statusTone(status: string) {
  if (status === "ok") return "text-emerald-700 bg-emerald-50 border-emerald-200";
  if (status === "disabled") return "text-slate-600 bg-slate-50 border-slate-200";
  return "text-amber-700 bg-amber-50 border-amber-200";
}

function groupOptions(groups: TunnelGroupSummary[], current: string) {
  const names = new Set(groups.map((group) => group.name));
  const options = [...groups];
  if (current && !names.has(current)) {
    options.push({ name: current, client_count: 0 });
  }
  return options;
}

export default function TunnelForwardingPage() {
  const { t } = useTranslation();
  const [loading, setLoading] = React.useState(true);
  const [saving, setSaving] = React.useState(false);
  const [rules, setRules] = React.useState<TunnelRule[]>([]);
  const [groups, setGroups] = React.useState<TunnelGroupSummary[]>([]);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [form, setForm] = React.useState<TunnelFormState>(EMPTY_FORM);

  const load = React.useCallback(async () => {
    setLoading(true);
    try {
      const payload = await getTunnelRules();
      setRules(payload.rules);
      setGroups(payload.groups);
    } catch (error) {
      toast.error(getReadableErrorMessage(error, t("tunnels.load_failed")));
    } finally {
      setLoading(false);
    }
  }, [t]);

  React.useEffect(() => {
    void load();
  }, [load]);

  const openCreate = () => {
    setForm({
      ...EMPTY_FORM,
      ingress_group: groups[0]?.name || "",
      egress_group: groups[0]?.name || "",
    });
    setDialogOpen(true);
  };

  const openEdit = (rule: TunnelRule) => {
    setForm(ruleToForm(rule));
    setDialogOpen(true);
  };

  const handleSave = async () => {
    let input: TunnelRuleInput;
    try {
      input = buildRuleInput(form);
      if (!input.ingress_group || !input.egress_group) {
        toast.error(t("tunnels.group_required"));
        return;
      }
      if (!input.target_host) {
        toast.error(t("tunnels.target_required"));
        return;
      }
    } catch (error) {
      toast.error(getReadableErrorMessage(error, t("tunnels.invalid_form")));
      return;
    }

    setSaving(true);
    try {
      const saved = await saveTunnelRule(input);
      setRules((current) => {
        const exists = current.some((item) => item.id === saved.id);
        return exists
          ? current.map((item) => (item.id === saved.id ? saved : item))
          : [saved, ...current];
      });
      setDialogOpen(false);
      toast.success(t("tunnels.save_success"));
      void load();
    } catch (error) {
      toast.error(getReadableErrorMessage(error, t("tunnels.save_failed")));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (rule: TunnelRule) => {
    if (!window.confirm(t("tunnels.delete_confirm", { name: rule.name }))) return;
    try {
      await deleteTunnelRule(rule.id);
      setRules((current) => current.filter((item) => item.id !== rule.id));
      toast.success(t("tunnels.delete_success"));
    } catch (error) {
      toast.error(getReadableErrorMessage(error, t("tunnels.delete_failed")));
    }
  };

  const canCreate = groups.length > 0;

  return (
    <AdminPageShell
      title={t("tunnels.title")}
      description={t("tunnels.description")}
    >
      <AdminPanel>
        <AdminPanelHeader
          title={t("tunnels.rules")}
          description={t("tunnels.rules_description")}
          action={(
            <Button onClick={openCreate} disabled={!canCreate}>
              {t("tunnels.create")}
            </Button>
          )}
        />
        <AdminPanelBody>
          {loading ? (
            <AdminTableSkeleton columns={9} rows={5} />
          ) : rules.length === 0 ? (
            <div className="rounded-md border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
              {canCreate ? t("tunnels.empty") : t("tunnels.no_groups")}
            </div>
          ) : (
            <AdminDataTable>
              <AdminDataTableHeader>
                <AdminDataTableRow>
                  <AdminDataTableHead>{t("common.status")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("common.name")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("tunnels.ingress_group")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("tunnels.listen")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("tunnels.egress_group")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("tunnels.target")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("tunnels.sessions")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("tunnels.last_error")}</AdminDataTableHead>
                  <AdminDataTableHead>{t("common.actions")}</AdminDataTableHead>
                </AdminDataTableRow>
              </AdminDataTableHeader>
              <AdminDataTableBody>
                {rules.map((rule) => (
                  <AdminDataTableRow key={rule.id}>
                    <AdminDataTableCell>
                      <Badge variant="outline" className={statusTone(rule.status)}>
                        {t(`tunnels.status.${rule.status}`, rule.status)}
                      </Badge>
                    </AdminDataTableCell>
                    <AdminDataTableCell className="font-medium">{rule.name}</AdminDataTableCell>
                    <AdminDataTableCell>{rule.ingress_group}</AdminDataTableCell>
                    <AdminDataTableCell>{rule.listen_address}:{rule.listen_port}</AdminDataTableCell>
                    <AdminDataTableCell>{rule.egress_group}</AdminDataTableCell>
                    <AdminDataTableCell>{rule.target_host}:{rule.target_port}</AdminDataTableCell>
                    <AdminDataTableCell>0</AdminDataTableCell>
                    <AdminDataTableCell>{rule.last_error || "-"}</AdminDataTableCell>
                    <AdminDataTableCell>
                      <div className="flex items-center gap-2">
                        <Button variant="outline" size="sm" onClick={() => openEdit(rule)}>
                          {t("common.edit")}
                        </Button>
                        <Button variant="destructive" size="sm" onClick={() => void handleDelete(rule)}>
                          {t("common.delete")}
                        </Button>
                      </div>
                    </AdminDataTableCell>
                  </AdminDataTableRow>
                ))}
              </AdminDataTableBody>
            </AdminDataTable>
          )}
        </AdminPanelBody>
      </AdminPanel>

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <Dialog.Content className="max-w-2xl">
          <Dialog.Header>
            <Dialog.Title>{form.id ? t("tunnels.edit") : t("tunnels.create")}</Dialog.Title>
            <Dialog.Description>{t("tunnels.form_description")}</Dialog.Description>
          </Dialog.Header>
          <div className="grid gap-4 py-2 sm:grid-cols-2">
            <div className="space-y-2">
              <Label>{t("common.name")}</Label>
              <Input value={form.name} onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))} />
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.enabled")}</Label>
              <div className="flex h-10 items-center">
                <Switch checked={form.enabled} onCheckedChange={(enabled) => setForm((current) => ({ ...current, enabled }))} />
              </div>
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.ingress_group")}</Label>
              <Select value={form.ingress_group} onValueChange={(value) => setForm((current) => ({ ...current, ingress_group: value }))}>
                {groupOptions(groups, form.ingress_group).map((group) => (
                  <Select.Item key={group.name} value={group.name}>
                    {group.name}{group.client_count === 0 ? ` · ${t("tunnels.group_empty")}` : ` · ${group.client_count}`}
                  </Select.Item>
                ))}
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.egress_group")}</Label>
              <Select value={form.egress_group} onValueChange={(value) => setForm((current) => ({ ...current, egress_group: value }))}>
                {groupOptions(groups, form.egress_group).map((group) => (
                  <Select.Item key={group.name} value={group.name}>
                    {group.name}{group.client_count === 0 ? ` · ${t("tunnels.group_empty")}` : ` · ${group.client_count}`}
                  </Select.Item>
                ))}
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.listen_address")}</Label>
              <Input value={form.listen_address} onChange={(event) => setForm((current) => ({ ...current, listen_address: event.target.value }))} />
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.listen_port")}</Label>
              <Input inputMode="numeric" value={form.listen_port} onChange={(event) => setForm((current) => ({ ...current, listen_port: event.target.value }))} />
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.target_host")}</Label>
              <Input value={form.target_host} onChange={(event) => setForm((current) => ({ ...current, target_host: event.target.value }))} />
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.target_port")}</Label>
              <Input inputMode="numeric" value={form.target_port} onChange={(event) => setForm((current) => ({ ...current, target_port: event.target.value }))} />
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.source_allowlist")}</Label>
              <Input value={form.source_allowlist} onChange={(event) => setForm((current) => ({ ...current, source_allowlist: event.target.value }))} />
            </div>
            <div className="space-y-2">
              <Label>{t("tunnels.max_sessions")}</Label>
              <Input inputMode="numeric" value={form.max_concurrent_sessions} onChange={(event) => setForm((current) => ({ ...current, max_concurrent_sessions: event.target.value }))} />
            </div>
            <div className="space-y-2 sm:col-span-2">
              <Label>{t("common.remark")}</Label>
              <Input value={form.remark} onChange={(event) => setForm((current) => ({ ...current, remark: event.target.value }))} />
            </div>
          </div>
          <Dialog.Footer>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>{t("common.cancel")}</Button>
            <Button onClick={() => void handleSave()} disabled={saving}>{saving ? t("common.saving") : t("common.save")}</Button>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog>
    </AdminPageShell>
  );
}
```

- [ ] **Step 4: Add route and menu**

In `src/routes.ts`, add under `/admin` children:

```ts
{
  path: "tunnels",
  element: React.createElement(lazy(() => import("./pages/admin/tunnels"))),
},
```

In `src/config/menuConfig.json`, add after the server entry:

```json
{
  "labelKey": "tunnels.title",
  "path": "/admin/tunnels",
  "icon": "Network"
}
```

- [ ] **Step 5: Add locale keys**

Add this `tunnels` object to `src/i18n/locales/zh_CN.json`:

```json
"tunnels": {
  "title": "隧道转发",
  "description": "按分组管理入口监听和出口目标，第一阶段仅保存规则和状态。",
  "rules": "隧道规则",
  "rules_description": "入口分组内的机器监听同一端口，出口分组后续会按连接轮询。",
  "create": "新增隧道",
  "edit": "编辑隧道",
  "form_description": "当前阶段只保存规则；KTP 数据面会在后续阶段启用。",
  "empty": "还没有隧道规则",
  "no_groups": "没有可用分组，请先把服务器加入分组。",
  "group_required": "请选择入口分组和出口分组",
  "target_required": "请填写目标地址",
  "invalid_form": "表单内容无效",
  "save_success": "隧道规则已保存",
  "save_failed": "保存隧道规则失败",
  "delete_confirm": "确认删除隧道规则「{{name}}」？",
  "delete_success": "隧道规则已删除",
  "delete_failed": "删除隧道规则失败",
  "load_failed": "加载隧道规则失败",
  "ingress_group": "入口分组",
  "egress_group": "出口分组",
  "listen": "监听",
  "target": "目标服务",
  "sessions": "连接数",
  "last_error": "最近错误",
  "enabled": "启用",
  "listen_address": "监听地址",
  "listen_port": "监听端口",
  "target_host": "目标地址",
  "target_port": "目标端口",
  "source_allowlist": "来源白名单",
  "max_sessions": "最大连接数",
  "group_empty": "无机器",
  "status": {
    "ok": "正常",
    "partial": "部分可用",
    "disabled": "已停用",
    "empty_ingress_group": "无入口机器",
    "empty_egress_group": "无出口机器",
    "unsupported_agent": "Agent 不支持",
    "listen_failed": "监听失败",
    "relay_unavailable": "Relay 不可用",
    "target_failed": "目标失败",
    "auth_failed": "认证失败"
  }
}
```

For `zh_TW.json`, use Traditional Chinese equivalents. For `en.json`, use English labels. For `ja_JP.json` and `id_ID.json`, use English fallback strings if localized copy is not already maintained in those files.

- [ ] **Step 6: Run web unit tests**

Run:

```powershell
node script\run-unit-tests.mjs
```

Expected: PASS including route/menu test.

- [ ] **Step 7: Run web build**

Run:

```powershell
npm run build
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```powershell
git add src/pages/admin/tunnels.tsx src/routes.ts src/config/menuConfig.json src/i18n/locales/zh_CN.json src/i18n/locales/zh_TW.json src/i18n/locales/en.json src/i18n/locales/ja_JP.json src/i18n/locales/id_ID.json script/run-unit-tests.mjs
git commit -m "Add tunnel forwarding admin page"
```

---

### Task 6: Integration Verification And Backend Pin

**Files:**
- Modify if needed: `C:\Users\Administrator\Documents\tanzhen\kelicloud\frontend-source.env`

- [ ] **Step 1: Run backend targeted tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./config -run "TestTunnelFeature|TestNormalizeAllowedFeatures"
go test ./database/tunnel
go test ./api/admin -run Test.*Tunnel
```

Expected: PASS. If `go` is not installed locally, run these through GitHub Actions after pushing and state that local Go verification was unavailable.

- [ ] **Step 2: Run web verification**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-web`:

```powershell
node script\run-unit-tests.mjs
npm run build
```

Expected: PASS.

- [ ] **Step 3: Push web first**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-web`:

```powershell
git status --short
git push origin radix
git rev-parse HEAD
```

Expected: status is clean except pre-existing untracked files, push succeeds, and the HEAD SHA is available for backend pinning.

- [ ] **Step 4: Pin latest web in backend**

In `C:\Users\Administrator\Documents\tanzhen\kelicloud\frontend-source.env`, set:

```env
KOMARI_PINNED_FRONTEND_REF=<web-head-sha>
```

Use the exact SHA printed by `git rev-parse HEAD`.

- [ ] **Step 5: Commit and push backend**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
git add frontend-source.env
git commit -m "Pin frontend with tunnel forwarding page"
git push origin main
```

Expected: push succeeds and backend GitHub Actions start.

- [ ] **Step 6: Verify GitHub Actions**

Check the backend commit workflow runs:

```powershell
$sha = "<backend-head-sha>"
Invoke-RestMethod -Uri "https://api.github.com/repos/keli-123456/kelicloud/actions/runs?head_sha=$sha&per_page=10" -Headers @{ 'User-Agent'='codex'; 'Accept'='application/vnd.github+json' }
```

Expected:

- `Build Binaries on Main Push and PR`: completed success.
- `Publish Docker Image on Main`: completed success.
- The Docker workflow step `Prepare frontend bundle` completed success.

- [ ] **Step 7: Record phase 1 result**

Add a short note to the implementation thread final response:

```text
Phase 1 delivered backend tunnel rule schema/API and the dedicated Tunnel Forwarding admin page. No agent KTP data-plane behavior was changed.
```

---

## Self-Review

Spec coverage:

- Dedicated **Tunnel Forwarding** page: Task 5.
- Group-only model with single-machine represented by one-member groups: Task 2 and Task 5.
- Empty groups hidden for create and preserved for existing rules: Task 2 and Task 5.
- Permission scope: Task 1 and Task 3.
- Port conflict strategy: Task 2.
- Status semantics: Task 2 and Task 3.
- No impact on current agent functions: Scope Check and Task 6.

Gaps deliberately left for future phase plans:

- KTP frame implementation.
- `/api/clients/tunnel` endpoint.
- Agent capability registration.
- Ingress listeners.
- Relay session pairing.
- TCP byte forwarding.

Type consistency:

- Backend model field names use snake_case JSON keys that match `src/lib/tunnels.ts`.
- Rule groups use existing `models.Client.Group` string values.
- Web route path is `/admin/tunnels`; backend API path is `/api/admin/tunnels`.
