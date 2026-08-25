*** Settings ***
Documentation       Verify 4.5.5.1 multi-attribute datasetId rules on input.
...
...                 Clause 4.5.5.1: 'If no datasetId is provided, or
...                 "datasetId": "@none" is supplied, it is considered as the
...                 default Attribute instance'; 'There can only be one default
...                 Attribute instance for an Attribute with a given Attribute
...                 name in any request or response'; 'The datasetId of the
...                 default Attribute instance is never explicitly included in
...                 responses'.
...
...                 Antares extension TP — official TPs cover the datasetId
...                 query filter (019_12) but not the "@none" input form nor
...                 the duplicate-default error path.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
455_01_01 Explicit @none DatasetId Is The Default Instance
    [Documentation]    4.5.5.1: "datasetId": "@none" designates the default
    ...    instance; the response default instance must NOT carry a datasetId.
    [Tags]    e-create    e-retrieve    4_5_5_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": [{"type": "Property", "value": 55, "datasetId": "@none"}, {"type": "Property", "value": 11, "datasetId": "urn:ngsi-ld:Dataset:gps"}]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${instances}=    Evaluate    $body['speed'] if isinstance($body['speed'], list) else [$body['speed']]
    Length Should Be    ${instances}    2
    ${default}=    Evaluate    [i for i in $instances if 'datasetId' not in i]
    Length Should Be    ${default}    1
    ${default_value}=    Evaluate    $default[0]['value']
    Should Be Equal As Integers    ${default_value}    55
    ${none_leak}=    Evaluate    [i for i in $instances if i.get('datasetId') == '@none']
    Should Be Empty    ${none_leak}
    [Teardown]    Delete Entity    ${entity_id}

455_01_02 Two Default Instances Are Rejected
    [Documentation]    4.5.5.1: only one default instance per Attribute name in
    ...    any request — absent datasetId plus "@none" is two defaults → 400.
    [Tags]    e-create    4_5_5_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": [{"type": "Property", "value": 55}, {"type": "Property", "value": 11, "datasetId": "@none"}]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

455_01_03 Duplicate DatasetId Is Rejected
    [Documentation]    4.5.5.1: instances of one Attribute are distinguished by
    ...    datasetId — a duplicated datasetId in one request is invalid → 400.
    [Tags]    e-create    4_5_5_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": [{"type": "Property", "value": 55, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 11, "datasetId": "urn:ngsi-ld:Dataset:a"}]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
