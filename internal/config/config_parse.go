package config

import "gopkg.in/yaml.v3"

// parsePresetField parses preset as string or list of strings.
func parsePresetField(node *yaml.Node) []string {
	if node == nil || node.Kind == 0 {
		return nil
	}
	switch node.Kind {
	case yaml.ScalarNode:
		if node.Value != "" {
			return []string{node.Value}
		}
	case yaml.SequenceNode:
		var result []string
		for _, item := range node.Content {
			if item.Kind == yaml.ScalarNode && item.Value != "" {
				result = append(result, item.Value)
			}
		}
		return result
	}
	return nil
}

// extractRepoOrder preserves YAML key ordering for repos and their services.
func extractRepoOrder(cfg *Config, root *yaml.Node) {
	if root.Kind != yaml.DocumentNode || len(root.Content) == 0 {
		return
	}
	doc := root.Content[0]
	if doc.Kind != yaml.MappingNode {
		return
	}

	for i := 0; i+1 < len(doc.Content); i += 2 {
		key := doc.Content[i].Value
		val := doc.Content[i+1]
		if (key == "repos" || key == "dirs") && val.Kind == yaml.MappingNode {
			for j := 0; j+1 < len(val.Content); j += 2 {
				repoName := val.Content[j].Value
				cfg.RepoOrder = append(cfg.RepoOrder, repoName)
				if dir, ok := cfg.Repos[repoName]; ok {
					svcNode := val.Content[j+1]
					if svcNode.Kind == yaml.MappingNode {
						for k := 0; k+1 < len(svcNode.Content); k += 2 {
							if svcNode.Content[k].Value == "services" && svcNode.Content[k+1].Kind == yaml.MappingNode {
								svcs := svcNode.Content[k+1]
								for s := 0; s+1 < len(svcs.Content); s += 2 {
									dir.ServiceOrder = append(dir.ServiceOrder, svcs.Content[s].Value)
								}
							}
						}
					}
				}
			}
		}
	}
}

func parseEnvFiles(node *yaml.Node) []EnvFileEntry {
	if node == nil || node.Kind == 0 {
		return nil
	}
	switch node.Kind {
	case yaml.ScalarNode:
		if node.Value == "" {
			return nil
		}
		return []EnvFileEntry{{File: node.Value}}
	case yaml.SequenceNode:
		var result []EnvFileEntry
		for _, item := range node.Content {
			switch item.Kind {
			case yaml.ScalarNode:
				result = append(result, EnvFileEntry{File: item.Value})
			case yaml.MappingNode:
				entry := EnvFileEntry{Env: make(map[string]string)}
				for i := 0; i+1 < len(item.Content); i += 2 {
					k := item.Content[i].Value
					v := item.Content[i+1]
					if k == "file" {
						entry.File = v.Value
					} else if k == "env" && v.Kind == yaml.MappingNode {
						for j := 0; j+1 < len(v.Content); j += 2 {
							entry.Env[v.Content[j].Value] = v.Content[j+1].Value
						}
					}
				}
				if entry.File != "" {
					result = append(result, entry)
				}
			}
		}
		return result
	}
	return nil
}

func parseSharedRefs(node *yaml.Node) []SharedServiceRef {
	if node == nil || node.Kind == 0 || node.Kind != yaml.SequenceNode {
		return nil
	}
	var result []SharedServiceRef
	for _, item := range node.Content {
		switch item.Kind {
		case yaml.ScalarNode:
			result = append(result, SharedServiceRef{Name: item.Value})
		case yaml.MappingNode:
			for i := 0; i+1 < len(item.Content); i += 2 {
				name := item.Content[i].Value
				ref := SharedServiceRef{Name: name}
				val := item.Content[i+1]
				if val.Kind == yaml.MappingNode {
					for j := 0; j+1 < len(val.Content); j += 2 {
						if val.Content[j].Value == "db_name" {
							ref.DBName = val.Content[j+1].Value
						}
					}
				}
				result = append(result, ref)
			}
		}
	}
	return result
}
