*** Settings ***
Documentation       Check that an NGSI-LD Null removes the scope in a Merge Entity
...                 operation. Clause 5.5.12: "For each member of the Fragment, whose
...                 value is an NGSI-LD Null, contained by the target, the target member
...                 is removed"; clause 4.18 admits "urn:ngsi-ld:null" as a scope for
...                 exactly this reason — it "shall be only used and only appear in case
...                 of deleted scopes", so it is the whole scope value or none of it.
...
...                 Antares extension TP — 056_03 covers the Attribute deletions, and
...                 no official TP deletes the scope.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
5512_01_01 A Null Scope Removes The Scope
    [Documentation]    5.5.12: the target member whose Fragment value is an NGSI-LD
    ...    Null is removed — the Entity comes back with no scope, the rest untouched.
    [Tags]    e-create    e-merge    e-retrieve    5_5_12    4_18    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "scope": "/Madrid/Gardens", "speed": {"type": "Property", "value": 55}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate
    ...    {"@context": [$ngsild_test_suite_context], "scope": "urn:ngsi-ld:null"}
    ${response}=    Merge Entity From JSON-LD Content    ${entity_id}    ${fragment}
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    Dictionary Should Not Contain Key    ${body}    scope
    ${speed}=    Evaluate    $body['speed']['value']
    Should Be Equal As Integers    ${speed}    55
    [Teardown]    Delete Entity    ${entity_id}

5512_01_02 A Null Mixed With A Scope Is Rejected
    [Documentation]    4.18: the sentinel appears only for deleted scopes, so beside a
    ...    real scope it is neither a deletion nor a scope → BadRequestData.
    [Tags]    e-create    e-merge    5_5_12    4_18    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "scope": "/Madrid/Gardens"}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate
    ...    {"@context": [$ngsild_test_suite_context], "scope": ["/Madrid", "urn:ngsi-ld:null"]}
    ${response}=    Merge Entity From JSON-LD Content    ${entity_id}    ${fragment}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    ${scope}=    Evaluate    $response.json()['scope']
    Should Be Equal As Strings    ${scope}    /Madrid/Gardens
    [Teardown]    Delete Entity    ${entity_id}
