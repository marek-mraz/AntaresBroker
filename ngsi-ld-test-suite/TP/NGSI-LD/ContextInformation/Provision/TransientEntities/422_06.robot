*** Settings ***
Documentation       4.22 × update-semantics asymmetry (Antares extension TP), per
...                 5.5.8 (p151-152) and 5.5.12: Update Attributes (5.6.2, PATCH
...                 /attrs/) replaces the WHOLE attribute (Example 2 removes the
...                 unmentioned unitCode) — so omitting expiresAt clears the expiry
...                 and the attribute becomes durable again. Partial Attribute
...                 Update (5.6.4, PATCH /attrs/{attrId}) overwrites only provided
...                 sub-members (Example 1 keeps unitCode) and Merge (5.5.12)
...                 likewise merges — both PRESERVE a stored expiresAt. Three
...                 transient attributes, same expiry, one updated per path: after
...                 expiry only the 5.6.2-replaced one survives.

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Teardown      Delete Entity    ${entity_id}


*** Variables ***
${EXPIRY_SECONDS}       ${5}


*** Test Cases ***
422_06_01 Whole Replace Clears Expiry Partial Update And Merge Preserve It
    [Tags]    transient    4_22
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    ${EXPIRY_SECONDS}s    result_format=%Y-%m-%dT%H:%M:%SZ
    ${payload}=    Catenate    SEPARATOR=${SPACE}
    ...    {"id": "${entity_id}", "type": "Building",
    ...    "temperature": {"type": "Property", "value": 1, "expiresAt": "${expiry}"},
    ...    "humidity": {"type": "Property", "value": 50, "expiresAt": "${expiry}"},
    ...    "pressure": {"type": "Property", "value": 1000, "expiresAt": "${expiry}"},
    ...    "@context": "${ngsild_test_suite_context}"}
    ${entity}=    Evaluate    json.loads($payload)    modules=json
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    # 5.6.2 Update Attributes: whole-attribute replace (5.5.8 Example 2) — the
    # replacement carries no expiresAt, so temperature becomes durable
    ${fragment}=    Evaluate
    ...    {"temperature": {"type": "Property", "value": 2}, "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    # 5.6.4 Partial Attribute Update: sub-member overwrite (5.5.8 Example 1
    # keeps unmentioned sub-members) — humidity keeps its expiresAt
    ${fragment}=    Evaluate
    ...    {"type": "Property", "value": 51, "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/humidity
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    # 5.5.12 Merge: merges into the instance — pressure keeps its expiresAt
    ${fragment}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "pressure": {"type": "Property", "value": 1001}, "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    # All three served before the (original) expiry
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    Should Be Equal As Numbers    ${body["temperature"]["value"]}    2
    Should Be Equal As Numbers    ${body["humidity"]["value"]}    51
    Should Be Equal As Numbers    ${body["pressure"]["value"]}    1001
    Sleep    ${EXPIRY_SECONDS + 2}s
    # After it: only the whole-replaced attribute survives, with its value
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    ${keys}=    Evaluate    sorted($body.keys())
    Should Be Equal As Numbers    ${body["temperature"]["value"]}    2
    Should Not Contain    ${keys}    humidity
    Should Not Contain    ${keys}    pressure
