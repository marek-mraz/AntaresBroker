*** Settings ***
Documentation       Verify 5.5.8 Partial Update Patch Behaviour.
...
...                 5.5.8: partial attribute update (5.6.4) overwrites at the
...                 SUB-attribute level (EXAMPLE 1: unitCode untouched);
...                 update attributes (5.6.2) replaces the WHOLE attribute
...                 (EXAMPLE 2: unitCode removed); an NGSI-LD Null value
...                 deletes the attribute (EXAMPLE 3); a datasetId cannot be
...                 deleted by setting it to "urn:ngsi-ld:null"; a Fragment
...                 instance with a new datasetId is ADDED, not replacing.
...
...                 Antares extension TP — official update TPs never assert
...                 the unitCode-preservation contrast nor the datasetId
...                 null prohibition.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temperature Entity
Test Teardown       Delete Entity    ${entity_id}


*** Keywords ***
Create Temperature Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "temperature": {"type": "Property", "value": 25, "unitCode": "CEL", "observedAt": "2022-03-14T01:59:26.535Z"}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}


*** Test Cases ***
558_01_01 Partial Attribute Update Overwrites Sub-Attributes Only
    [Documentation]    5.5.8 EXAMPLE 1: PATCH attrs/{attrId} overwrites value
    ...    and observedAt but leaves unitCode untouched.
    [Tags]    ea-partial-update    5_5_8    since_v1.9.1
    ${fragment}=    Evaluate
    ...    {"type": "Property", "value": 100, "observedAt": "2022-03-14T13:00:00.000Z"}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/temperature
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    ${t}=    Evaluate    $response.json()['temperature']
    Should Be Equal As Integers    ${t['value']}    100
    Should Be Equal    ${t['observedAt']}    2022-03-14T13:00:00.000Z
    Should Be Equal    ${t['unitCode']}    CEL

558_01_02 Update Attributes Replaces The Whole Attribute
    [Documentation]    5.5.8 EXAMPLE 2: PATCH attrs/ replaces the whole
    ...    temperature Attribute — unitCode is REMOVED.
    [Tags]    ea-update    5_5_8    since_v1.9.1
    ${fragment}=    Evaluate
    ...    {"temperature": {"type": "Property", "value": 100, "observedAt": "2022-03-14T13:00:00.000Z"}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    ${t}=    Evaluate    $response.json()['temperature']
    Should Be Equal As Integers    ${t['value']}    100
    Dictionary Should Not Contain Key    ${t}    unitCode
    Should Not Contain    ${response.text}    CEL

558_01_03 NGSI-LD Null Value Deletes The Attribute
    [Documentation]    5.5.8 EXAMPLE 3: an update with value urn:ngsi-ld:null
    ...    deletes the whole Attribute; other members stay untouched.
    [Tags]    ea-update    5_5_8    since_v1.9.1
    ${fragment}=    Evaluate
    ...    {"temperature": {"type": "Property", "value": "urn:ngsi-ld:null"}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.json()}    temperature
    Should Not Contain    ${response.text}    urn:ngsi-ld:null

558_01_04 DatasetId Cannot Be Deleted Via Null And New DatasetId Adds
    [Documentation]    5.5.8: "A datasetId cannot be deleted by setting it to
    ...    the value urn:ngsi-ld:null" → 400; a Fragment instance carrying a
    ...    NEW datasetId is added as a new instance, not a replacement.
    [Tags]    ea-partial-update    ea-update    5_5_8    since_v1.9.1
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${fragment}=    Evaluate    {"datasetId": "urn:ngsi-ld:null"}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/temperature
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    # a new datasetId is ADDED alongside the default instance
    ${fragment}=    Evaluate
    ...    {"temperature": {"type": "Property", "value": 30, "datasetId": "urn:ngsi-ld:Dataset:extra"}}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    ${t}=    Evaluate    $response.json()['temperature']
    ${count}=    Get Length    ${t}
    Should Be Equal As Integers    ${count}    2
    ${values}=    Evaluate    sorted(i['value'] for i in $t)
    Should Be Equal    ${values}    ${{[25, 30]}}
