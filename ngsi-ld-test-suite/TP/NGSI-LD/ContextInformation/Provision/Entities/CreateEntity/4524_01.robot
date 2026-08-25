*** Settings ***
Documentation       Verify the 4.5.24 JsonProperty representations.
...
...                 4.5.24.2/3: "json" is a raw JSON object (or array of objects)
...                 "never subject to JSON-LD term expansion or compaction";
...                 "unitCode: shall never be present, as raw JSON objects are
...                 unitless"; "value ... shall never be present, as value is a
...                 generalization of json". 4.5.24.3: type may be omitted —
...                 "JsonProperty can be inferred by the presence of the json
...                 attribute".
...
...                 Antares extension TP — official JsonProperty TPs (001_14,
...                 010_10, 012_08, 020_16, 046_32/35/38) cover only happy paths;
...                 none cover the prohibited members, invalid json shapes, or
...                 concise type inference.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
4524_01_01 Concise JsonProperty Is Inferred And Json Kept Verbatim
    [Documentation]    4.5.24.3: type inferred from the json member; the raw JSON
    ...    object round-trips verbatim — inner keys that are @context terms
    ...    (speed, brandName) must NOT be expanded or compacted, and the
    ...    normalized output must NOT grow value or unitCode members.
    [Tags]    e-create    e-retrieve    4_5_24_2    4_5_24_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "tickets": {"json": {"speed": 3, "brandName": "x", "type": "raw"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${tickets}=    Evaluate    $response.json()['tickets']
    ${ttype}=    Evaluate    $tickets['type']
    Should Be Equal    ${ttype}    JsonProperty
    ${expected}=    Evaluate    {"speed": 3, "brandName": "x", "type": "raw"}
    Should Be Equal    ${tickets['json']}    ${expected}
    Should Not Contain    ${tickets}    value
    Should Not Contain    ${tickets}    unitCode
    [Teardown]    Delete Entity    ${entity_id}

4524_01_02 JsonProperty With UnitCode Is Rejected
    [Documentation]    4.5.24.2: "unitCode: shall never be present, as raw JSON
    ...    objects are unitless" → 400 BadRequestData.
    [Tags]    e-create    4_5_24_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "tickets": {"type": "JsonProperty", "json": {"a": 1}, "unitCode": "MTR"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

4524_01_03 JsonProperty With A Value Member Is Rejected
    [Documentation]    4.5.24.2: "value ... shall never be present, as value is a
    ...    generalization of json" → 400 BadRequestData.
    [Tags]    e-create    4_5_24_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "tickets": {"type": "JsonProperty", "json": {"a": 1}, "value": 1}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

4524_01_04 Scalar Json Member Is Rejected
    [Documentation]    4.5.24.2: json is "a raw JSON object (or array of objects)" —
    ...    a scalar is invalid content → 400 BadRequestData.
    [Tags]    e-create    4_5_24_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "tickets": {"type": "JsonProperty", "json": 5}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
