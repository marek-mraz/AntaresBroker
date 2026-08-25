package com.testrunner.service.robot;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.*;
import com.gitb.core.*;

import java.util.*;

import org.springframework.stereotype.Service;

@Service
public class RobotListenerResultParser {

    private static final ObjectMapper mapper = new ObjectMapper();

    private static final Set<String> RAW_FIELDS =
        Set.of("body");

    public AnyContent parseToAnyContent(String listenerJson) {

        try {

            JsonNode root = mapper.readTree(listenerJson);
            AnyContent output = new AnyContent();
            output.setName("response");

            Map<String, Integer> sectionCounters = new HashMap<>();

            Iterator<Map.Entry<String, JsonNode>> sections = root.fields();

            while (sections.hasNext()) {

                Map.Entry<String, JsonNode> sectionEntry = sections.next();

                String sectionName = sectionEntry.getKey().toLowerCase();
                JsonNode arrayNode = sectionEntry.getValue();

                if (!arrayNode.isArray()) continue;

                int sectionIndex = sectionCounters.getOrDefault(sectionName, 0);

                for (JsonNode keywordNode : arrayNode) {

                    JsonNode argsNode = keywordNode.get("args");
                    JsonNode statusNode = keywordNode.get("status");

                    if (argsNode == null || statusNode == null) continue;

                    AnyContent argsContent =
                            jsonNodeToAnyContent("args", argsNode);

                    AnyContent statusContent = new AnyContent();
                    statusContent.setName("status");

                    String status = statusNode.asText(null);

                    if ("PASS".equalsIgnoreCase(status)) {
                        statusContent.setValue("SUCCESS");
                    }
                    else if ("FAIL".equalsIgnoreCase(status)) {
                        statusContent.setValue("FAILURE");
                    }
                    else if (status == null || status.isBlank() || "NOT RUN".equalsIgnoreCase(status)) { // Not sure about the "NOT RUN" condition
                        statusContent.setValue("UNKNOWN");
                    }
                    else {
                        statusContent.setValue(status);
                    }

                    statusContent.setEmbeddingMethod(
                            ValueEmbeddingEnumeration.STRING
                    );

                    AnyContent keywordOutput = new AnyContent();
                    keywordOutput.setName(sectionName + sectionIndex);
                    sectionIndex++;
                    keywordOutput.getItem().add(argsContent);
                    keywordOutput.getItem().add(statusContent);

                    output.getItem().add(keywordOutput);
                }
                sectionCounters.put(sectionName, sectionIndex);
            }
            String json = mapper
                .writerWithDefaultPrettyPrinter()
                .writeValueAsString(output);
            System.err.println("[OUTPUT]\n" + json + "\n[OUTPUT]");

            return output;

        } catch (Exception e) {
            throw new RuntimeException(
                    "Failed to parse listener JSON", e
            );
        }
    }

    private static AnyContent jsonNodeToAnyContent(
            String name, JsonNode node) {

        AnyContent content = new AnyContent();
        content.setName(name);

        if (RAW_FIELDS.contains(name)) {

            try {
                String prettyJson = mapper
                                .writerWithDefaultPrettyPrinter()
                                .writeValueAsString(node);

                content.setValue(prettyJson);
            } catch (JsonProcessingException e) {
                content.setValue(node.toString());
            }

            content.setEmbeddingMethod(
                ValueEmbeddingEnumeration.STRING
            );

            return content;
        }

        if (node.isObject()) {

            node.fields().forEachRemaining(entry -> {
                AnyContent child =
                        jsonNodeToAnyContent(
                                entry.getKey(),
                                entry.getValue()
                        );
                content.getItem().add(child);
            });

            if (content.getItem().isEmpty()) {
                content.setValue("{}");
                content.setEmbeddingMethod(
                        ValueEmbeddingEnumeration.STRING
                );
            }
        }
        else {

            String value = node.asText("");

            content.setValue(value);
            content.setEmbeddingMethod(
                    ValueEmbeddingEnumeration.STRING
            );

        }

        return content;
    }

}