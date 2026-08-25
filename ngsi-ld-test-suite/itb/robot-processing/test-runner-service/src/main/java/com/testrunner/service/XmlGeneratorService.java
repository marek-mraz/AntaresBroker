package com.testrunner.service;

import com.testrunner.model.itb.Scenario;
import com.testrunner.model.robot.*;

import freemarker.template.*;
import jakarta.annotation.PostConstruct;

import org.springframework.stereotype.Service;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.stream.Collectors;

@Service
public class XmlGeneratorService {

    private Configuration freemarkerConfig;

    @PostConstruct
    public void setup() {
        freemarkerConfig = new Configuration(Configuration.VERSION_2_3_31);
        freemarkerConfig.setClassLoaderForTemplateLoading(
            getClass().getClassLoader(),
            "itb-templates"
        );
        freemarkerConfig.setDefaultEncoding("UTF-8");
    }
    
    private String generateBlock(String templateName, Map<String, Object> dataModel) throws IOException, TemplateException {
        Template template = freemarkerConfig.getTemplate(templateName);
        StringWriter writer = new StringWriter();
        template.process(dataModel, writer);
        return writer.toString();
    }

    public void generateXmlTestCase(Scenario scenario) throws IOException, TemplateException {

        Map<String, Object> data = new HashMap<>();
        data.put("dynamicTestId", scenario.getTestId());
        data.put("dynamicTestName", scenario.getTestName());
        data.put("dynamicDescription", scenario.getDescription());

        StringBuilder xml = new StringBuilder();

        // The root template should be the beginning of every generated ITB XML
        xml.append(generateBlock("root.xml", data));

        List<String> actors = scenario.getActors();

        // For clarity, we want the actors displayed in the ITB diagram like : Consumer - SUT - Producer
        // By default they will appear following the chronological order they will appear in the test.
        // And everytime the first actor involved is the SUT behind which the first step is placed.
        // That's why for the first test actor and for it only we need to add this displayOrder attribute.
        if (!actors.isEmpty()) {
            String firstActor = actors.get(0);
            data.put("dynamicDisplayOrder", " displayOrder='0'");
            xml.append(generateBlock("actor-" + firstActor + ".xml", data));
        }

        data.put("dynamicDisplayOrder", "");

        xml.append(generateBlock("actor-ContextBroker.xml", data));

        for (int i = 1; i < actors.size(); i++) {
            String actor = actors.get(i);
            if (actors != null && actors.contains(actor)) {
                xml.append(generateBlock("actor-" + actor + ".xml", data));
            }
        }

        xml.append(generateBlock("start-steps.xml", data));
        
        List<KeywordStep> setupKeywords = scenario.getSetup();
        if (setupKeywords != null && !setupKeywords.isEmpty()) {
            xml.append(generateBlock("start-setup.xml", data));
            appendKeywordBlocks(setupKeywords, data, xml, "setup");
            xml.append(generateBlock("end-setup.xml", data));
        }
        
        // After the further modification of the listeners to catch the "Wait For" keywords
        // The "if" condition will be removed as a test will always has keywords.
        List<KeywordStep> testKeywords = scenario.getSteps();
        if (testKeywords != null && !testKeywords.isEmpty()) {
            xml.append(generateBlock("start-test.xml", data));
            appendKeywordBlocks(testKeywords, data, xml, "test");
            xml.append(generateBlock("end-test.xml", data));
        }

        List<KeywordStep> teardownKeywords = scenario.getTeardown();
        if (teardownKeywords != null && !teardownKeywords.isEmpty()) {
            xml.append(generateBlock("start-teardown.xml", data));
            appendKeywordBlocks(teardownKeywords, data, xml, "teardown");
            xml.append(generateBlock("end-teardown.xml", data));
        }

        xml.append(generateBlock("close-tags.xml", data));

        writeXmlTestCaseFile(scenario, xml.toString());
    }

    public void generateXmlTestSuiteFile(Path dir) throws IOException, TemplateException {

        StringBuilder xml = new StringBuilder();

        Map<String, Object> rootData = new HashMap<>();

        rootData.put("dynamicSuiteName", dir.getFileName().toString());
        xml.append(generateBlock("test-suite-root.xml", rootData));

        List<String> testIds = getFileNamesWithoutExtension(dir);
        testIds.sort(String.CASE_INSENSITIVE_ORDER);
        for (String testId : testIds) {
            Map<String, Object> data = new HashMap<>();
            data.put("dynamicTestId", testId);
            xml.append(generateBlock("test-suite-case.xml", data));
        }

        xml.append(generateBlock("test-suite-close-tags.xml", rootData));

        Path outputFile = dir.resolve("testSuite.xml");

        Files.writeString(outputFile, xml, StandardCharsets.UTF_8);
    }

    private void writeXmlTestCaseFile(Scenario scenario, String xmlContent) throws IOException {
        Path baseDir = Paths.get("/opt/generated-itb-xml");
        
        String fullPath = scenario.getPath();

        // In the default Robot repository, the tests are NGSI-LD/A/B/C
        // And in ITB we want all the tests in the subfolder "A" packed together
        String dirPart = truncatePath(fullPath, 1 ,2);

        String dirName = dirPart.replace("/", "_");

        String fileName = scenario.getTestId();

        fileName += ".xml";

        Path outputDir = baseDir.resolve(dirName);
        Files.createDirectories(outputDir);

        Path outputFile = outputDir.resolve(fileName);

        Files.writeString(outputFile, xmlContent, StandardCharsets.UTF_8);
    }

    // If the ITB test suites architecture should be different than the Robot one,
    // this method will be useful.
    private String truncatePath(String path, int startIndex, int endIndex) {

        if (path == null || path.isBlank()) {
            return "";
        }

        String[] parts = path.split("/");

        if (startIndex < 0) {
            startIndex = 0;
        }
        if (endIndex > parts.length) {
            endIndex = parts.length;
        }
        if (startIndex >= endIndex) {
            return "";
        }

        StringBuilder result = new StringBuilder();

        for (int i = startIndex; i < endIndex; i++) {
            result.append(parts[i]);
            if (i < endIndex - 1) {
                result.append("/");
            }
        }

        return result.toString();
    }

    private String selectTemplateForKeyword(KeywordStep keyword) {
        switch (keyword.getKeyword().toUpperCase()) {
            case "POST", "GET", "DELETE", "PATCH", "PUT":
                return "request.xml";
            default :
                return "check.xml";
        }
    }
    
    private void appendKeywordBlocks(
            List<KeywordStep> keywords,
            Map<String, Object> data,
            StringBuilder xml,
            String step
    ) throws IOException, TemplateException {

        AtomicInteger stepCounter = new AtomicInteger(0);

        for (KeywordStep keyword : keywords) {
            data.put("dynamicIndentation", step + stepCounter.getAndIncrement());
            data.put("dynamicKeyword", keyword.getKeyword());
            data.put("dynamicActor", keyword.getActor() != null ? keyword.getActor() : "ITB");

            String template = selectTemplateForKeyword(keyword);
            xml.append(generateBlock(template, data));
        }
    }

    public List<String> getFileNamesWithoutExtension(Path folder) throws IOException {
        if (!Files.isDirectory(folder)) {
            throw new IllegalArgumentException("Path must be a directory");
        }

        return Files.list(folder)
                .filter(Files::isRegularFile)
                .map(path -> path.getFileName().toString())
                .map(name -> {
                    int dotIndex = name.lastIndexOf('.');
                    return (dotIndex > 0) ? name.substring(0, dotIndex) : name;
                })
                .collect(Collectors.toList());
    }
}