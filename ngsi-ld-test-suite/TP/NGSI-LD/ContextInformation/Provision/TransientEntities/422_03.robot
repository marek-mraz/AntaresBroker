*** Settings ***
Documentation       4.22 transient ATTRIBUTES (Antares extension TP). Per Tables
...                 5.2.5-1 and 5.2.6-1 both Property and Relationship carry a
...                 settable expiresAt scoped to the storage of that attribute:
...                 a transient Property, a transient Relationship, and one
...                 transient instance of a multi-instance (datasetId) Property
...                 all vanish at expiry while their durable siblings — and the
...                 entity itself — survive. All provided at CREATE time
...                 (422_01_05 covers the append path).

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Entity With Transient Attributes
Suite Teardown      Delete Entity    ${entity_id}


*** Variables ***
${EXPIRY_SECONDS}       ${6}


*** Test Cases ***
422_03_01 Transient Attributes Are Served Before Expiry
    [Tags]    transient    4_22
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    ${keys}=    Evaluate    sorted($body.keys())
    FOR    ${attr}    IN    temperature    humidity    ownedBy    locatedIn    speed
        Should Contain    ${keys}    ${attr}
    END
    ${speeds}=    Evaluate    sorted(i["value"] for i in $body["speed"])
    Should Be Equal    ${speeds}    ${{["durable-speed", "ephemeral-speed"]}}    both datasetId instances served

422_03_02 Expired Attributes Vanish Durable Siblings And Entity Survive
    [Tags]    transient    4_22
    Sleep    ${EXPIRY_SECONDS + 2}s
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    ${keys}=    Evaluate    sorted($body.keys())
    # transient Property and transient Relationship gone as members
    Should Not Contain    ${keys}    temperature
    Should Not Contain    ${keys}    ownedBy
    # transient datasetId instance gone, durable sibling kept — exactly one
    # instance remains and it is the durable one
    ${speed}=    Evaluate    $body["speed"] if isinstance($body["speed"], list) else [$body["speed"]]
    Length Should Be    ${speed}    1
    Should Be Equal    ${speed[0]["value"]}    durable-speed
    # durable attributes untouched, exact values
    Should Be Equal As Numbers    ${body["humidity"]["value"]}    60
    Should Be Equal    ${body["locatedIn"]["object"]}    urn:ngsi-ld:City:bb


*** Keywords ***
Create Entity With Transient Attributes
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    ${EXPIRY_SECONDS}s    result_format=%Y-%m-%dT%H:%M:%SZ
    ${payload}=    Catenate    SEPARATOR=${SPACE}
    ...    {"id": "${entity_id}", "type": "Building",
    ...    "temperature": {"type": "Property", "value": 21, "expiresAt": "${expiry}"},
    ...    "humidity": {"type": "Property", "value": 60},
    ...    "ownedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:owner1", "expiresAt": "${expiry}"},
    ...    "locatedIn": {"type": "Relationship", "object": "urn:ngsi-ld:City:bb"},
    ...    "speed": [
    ...    {"type": "Property", "value": "ephemeral-speed", "datasetId": "urn:ngsi-ld:dataset:gps", "expiresAt": "${expiry}"},
    ...    {"type": "Property", "value": "durable-speed", "datasetId": "urn:ngsi-ld:dataset:odometer"}],
    ...    "@context": "${ngsild_test_suite_context}"}
    ${entity}=    Evaluate    json.loads($payload)    modules=json
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
