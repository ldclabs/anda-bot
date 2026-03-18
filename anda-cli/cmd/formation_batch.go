package cmd

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
)

const defaultBatchReportFileName = ".formation-batch-checklist.json"

const (
	batchStatusPending   = "pending"
	batchStatusWorking   = "working"
	batchStatusSucceeded = "succeeded"
	batchStatusFailed    = "failed"
)

type fileFormationBatchOptions struct {
	RootDir      string
	FileName     string
	Extension    string
	ReportPath   string
	RetryFailed  bool
	DryRun       bool
	InputContext *api.InputContext
}

type fileFormationChecklist struct {
	RootDir   string                                  `json:"root_dir"`
	Selector  string                                  `json:"selector"`
	UpdatedAt string                                  `json:"updated_at"`
	Entries   map[string]*fileFormationChecklistEntry `json:"entries"`
}

type fileFormationChecklistEntry struct {
	Path         string `json:"path"`
	Status       string `json:"status"`
	Attempts     int    `json:"attempts"`
	LastError    string `json:"last_error,omitempty"`
	UpdatedAt    string `json:"updated_at"`
	Conversation *int   `json:"conversation,omitempty"`
}

func runFileFormationBatch(ctx context.Context, client *api.Client, opts fileFormationBatchOptions) error {
	if strings.TrimSpace(opts.RootDir) == "" {
		return fmt.Errorf("--batch-dir cannot be empty")
	}

	absRootDir, err := filepath.Abs(opts.RootDir)
	if err != nil {
		return fmt.Errorf("resolve batch dir: %w", err)
	}

	selector, err := resolveBatchSelector(opts.FileName, opts.Extension)
	if err != nil {
		return err
	}

	reportPath, err := resolveBatchReportPath(absRootDir, opts.ReportPath)
	if err != nil {
		return err
	}

	targetFiles, err := findBatchFiles(absRootDir, selector)
	if err != nil {
		return err
	}
	if len(targetFiles) == 0 {
		return fmt.Errorf("no files matched selector %q under %q", selector, absRootDir)
	}

	checklist, err := loadFileFormationChecklist(reportPath, absRootDir, selector)
	if err != nil {
		return err
	}
	mergeChecklistEntries(checklist, absRootDir, targetFiles)
	if err := saveFileFormationChecklist(reportPath, checklist); err != nil {
		return err
	}

	total := len(targetFiles)
	processed := 0
	skipped := 0
	wouldProcess := 0
	succeeded := 0
	failed := 0

	for idx, targetFile := range targetFiles {
		if ctx.Err() != nil {
			return ctx.Err()
		}

		relPath, err := filepath.Rel(absRootDir, targetFile)
		if err != nil {
			return fmt.Errorf("resolve relative path for %q: %w", targetFile, err)
		}

		entry := checklist.Entries[relPath]
		if !shouldProcessBatchEntry(entry, opts.RetryFailed) {
			skipped++
			fmt.Printf("[%d/%d] Skip %s (status=%s)\n", idx+1, total, relPath, entry.Status)
			continue
		}

		processed++
		if opts.DryRun {
			wouldProcess++
			fmt.Printf("[%d/%d] DRY  %s\n", idx+1, total, relPath)
			continue
		}

		entry.Attempts++
		entry.Status = batchStatusWorking
		entry.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
		if err := saveFileFormationChecklist(reportPath, checklist); err != nil {
			return err
		}

		content, err := os.ReadFile(targetFile)
		if err != nil {
			markBatchFormationFailed(entry, fmt.Sprintf("read file: %v", err))
			if saveErr := saveFileFormationChecklist(reportPath, checklist); saveErr != nil {
				return saveErr
			}
			failed++
			fmt.Printf("[%d/%d] Fail %s: %v\n", idx+1, total, relPath, err)
			continue
		}

		messages, err := parseMessagesInput(string(content))
		if err != nil {
			markBatchFormationFailed(entry, fmt.Sprintf("parse messages: %v", err))
			if saveErr := saveFileFormationChecklist(reportPath, checklist); saveErr != nil {
				return saveErr
			}
			failed++
			fmt.Printf("[%d/%d] Fail %s: %v\n", idx+1, total, relPath, err)
			continue
		}

		if err := validateMessageContentLength(messages); err != nil {
			markBatchFormationFailed(entry, err.Error())
			if saveErr := saveFileFormationChecklist(reportPath, checklist); saveErr != nil {
				return saveErr
			}
			failed++
			fmt.Printf("[%d/%d] Fail %s: %v\n", idx+1, total, relPath, err)
			continue
		}

		input := &api.FormationInput{
			Messages:  messages,
			Timestamp: time.Now().UTC().Format(time.RFC3339),
			Context:   opts.InputContext,
		}

		resp, err := client.Formation(ctx, input)
		if err != nil {
			markBatchFormationFailed(entry, err.Error())
			if saveErr := saveFileFormationChecklist(reportPath, checklist); saveErr != nil {
				return saveErr
			}
			failed++
			fmt.Printf("[%d/%d] Fail %s: %v\n", idx+1, total, relPath, err)
			if ctx.Err() != nil {
				return ctx.Err()
			}
			continue
		}
		if resp.Error != nil {
			markBatchFormationFailed(entry, resp.Error.Error())
			if saveErr := saveFileFormationChecklist(reportPath, checklist); saveErr != nil {
				return saveErr
			}
			failed++
			fmt.Printf("[%d/%d] Fail %s: %v\n", idx+1, total, relPath, resp.Error)
			continue
		}

		entry.Status = batchStatusSucceeded
		entry.LastError = ""
		entry.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
		entry.Conversation = nil
		if resp.Result != nil {
			entry.Conversation = resp.Result.Conversation
		}
		if err := saveFileFormationChecklist(reportPath, checklist); err != nil {
			return err
		}

		succeeded++
		fmt.Printf("[%d/%d] OK   %s\n", idx+1, total, relPath)
	}

	if opts.DryRun {
		fmt.Printf("Batch dry-run done. total=%d matched=%d would_submit=%d skipped=%d checklist=%s\n", total, processed, wouldProcess, skipped, reportPath)
		return nil
	}

	fmt.Printf("Batch done. total=%d processed=%d succeeded=%d failed=%d skipped=%d checklist=%s\n", total, processed, succeeded, failed, skipped, reportPath)
	if failed > 0 {
		return fmt.Errorf("batch finished with %d failures, see checklist %q", failed, reportPath)
	}
	return nil
}

func resolveBatchSelector(fileName, extension string) (string, error) {
	fileName = strings.TrimSpace(fileName)
	extension = strings.TrimSpace(extension)

	if fileName == "" && extension == "" {
		return "", fmt.Errorf("batch selector is required: set --batch-file-name or --batch-ext")
	}
	if fileName != "" && extension != "" {
		return "", fmt.Errorf("--batch-file-name and --batch-ext cannot be used together")
	}

	if fileName != "" {
		return "name:" + strings.ToLower(fileName), nil
	}

	if !strings.HasPrefix(extension, ".") {
		extension = "." + extension
	}
	if extension == "." {
		return "", fmt.Errorf("invalid --batch-ext value")
	}
	return "ext:" + strings.ToLower(extension), nil
}

func resolveBatchReportPath(rootDir, reportPath string) (string, error) {
	if strings.TrimSpace(reportPath) == "" {
		return filepath.Join(rootDir, defaultBatchReportFileName), nil
	}

	absReportPath, err := filepath.Abs(reportPath)
	if err != nil {
		return "", fmt.Errorf("resolve batch report path: %w", err)
	}
	return absReportPath, nil
}

func findBatchFiles(rootDir, selector string) ([]string, error) {
	files := make([]string, 0)
	err := filepath.WalkDir(rootDir, func(path string, d fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if d.IsDir() {
			return nil
		}
		if matchesBatchSelector(d.Name(), selector) {
			files = append(files, path)
		}
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("scan batch dir %q: %w", rootDir, err)
	}
	sort.Strings(files)
	return files, nil
}

func matchesBatchSelector(fileName, selector string) bool {
	if strings.HasPrefix(selector, "name:") {
		name := strings.TrimPrefix(selector, "name:")
		return strings.EqualFold(fileName, name)
	}
	if strings.HasPrefix(selector, "ext:") {
		ext := strings.TrimPrefix(selector, "ext:")
		return strings.EqualFold(filepath.Ext(fileName), ext)
	}
	return false
}

func loadFileFormationChecklist(reportPath, expectedRootDir, expectedSelector string) (*fileFormationChecklist, error) {
	data, err := os.ReadFile(reportPath)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return &fileFormationChecklist{
				RootDir:  expectedRootDir,
				Selector: expectedSelector,
				Entries:  make(map[string]*fileFormationChecklistEntry),
			}, nil
		}
		return nil, fmt.Errorf("read checklist %q: %w", reportPath, err)
	}

	var checklist fileFormationChecklist
	if err := json.Unmarshal(data, &checklist); err != nil {
		return nil, fmt.Errorf("parse checklist %q: %w", reportPath, err)
	}
	if checklist.Entries == nil {
		checklist.Entries = make(map[string]*fileFormationChecklistEntry)
	}
	if checklist.RootDir == "" {
		checklist.RootDir = expectedRootDir
	}
	if checklist.RootDir != expectedRootDir {
		return nil, fmt.Errorf("checklist root_dir mismatch: expected %q, got %q", expectedRootDir, checklist.RootDir)
	}
	if checklist.Selector == "" {
		checklist.Selector = expectedSelector
	}
	if checklist.Selector != expectedSelector {
		return nil, fmt.Errorf("checklist selector mismatch: expected %q, got %q", expectedSelector, checklist.Selector)
	}
	return &checklist, nil
}

func saveFileFormationChecklist(reportPath string, checklist *fileFormationChecklist) error {
	checklist.UpdatedAt = time.Now().UTC().Format(time.RFC3339)

	if err := os.MkdirAll(filepath.Dir(reportPath), 0o755); err != nil {
		return fmt.Errorf("create checklist directory: %w", err)
	}

	data, err := json.MarshalIndent(checklist, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal checklist: %w", err)
	}

	tmpPath := reportPath + ".tmp"
	if err := os.WriteFile(tmpPath, data, 0o644); err != nil {
		return fmt.Errorf("write checklist temp file: %w", err)
	}
	if err := os.Rename(tmpPath, reportPath); err != nil {
		return fmt.Errorf("replace checklist file: %w", err)
	}
	return nil
}

func mergeChecklistEntries(checklist *fileFormationChecklist, rootDir string, targetFiles []string) {
	for _, targetFile := range targetFiles {
		relPath, err := filepath.Rel(rootDir, targetFile)
		if err != nil {
			continue
		}
		if _, exists := checklist.Entries[relPath]; exists {
			continue
		}
		checklist.Entries[relPath] = &fileFormationChecklistEntry{
			Path:      relPath,
			Status:    batchStatusPending,
			UpdatedAt: time.Now().UTC().Format(time.RFC3339),
		}
	}
}

func shouldProcessBatchEntry(entry *fileFormationChecklistEntry, retryFailed bool) bool {
	if entry == nil {
		return true
	}
	switch entry.Status {
	case batchStatusSucceeded:
		return false
	case batchStatusFailed:
		return retryFailed
	default:
		return true
	}
}

func markBatchFormationFailed(entry *fileFormationChecklistEntry, errMsg string) {
	entry.Status = batchStatusFailed
	entry.LastError = errMsg
	entry.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
}
