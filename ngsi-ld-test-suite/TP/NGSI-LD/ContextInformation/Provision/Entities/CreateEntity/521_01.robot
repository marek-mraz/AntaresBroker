*** Settings ***
Documentation       Check the NGSI-LD Null usage restriction (CIM 009 clause 5.2.1):
...                 "in the context of a partial update or merge operation ... an
...                 NGSI-LD Null shall be used to indicate the removal of a target
...                 member ... In all other cases, implementations shall raise an error
...                 of type BadRequestData if an NGSI-LD Null value is encountered."
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity With Attribute Expecting 400


*** Test Cases ***    ATTR_JSON
521_01_01 A Null Property Value Cannot Be Created
    [Documentation]    5.2.1: the NGSI-LD Null on a create input is BadRequestData
    [Tags]    e-create    5_2_1    since_v1.9.1
    {"type": "Property", "value": "urn:ngsi-ld:null"}

521_01_02 A Null Relationship Object Cannot Be Created
    [Documentation]    5.2.1: same rule for a Relationship object
    [Tags]    e-create    5_2_1    since_v1.9.1
    {"type": "Relationship", "object": "urn:ngsi-ld:null"}

521_01_03 A Null Sub-Attribute Value Cannot Be Created
    [Documentation]    5.2.1: nested members are covered too
    [Tags]    e-create    5_2_1    since_v1.9.1
    {"type": "Property", "value": 1, "sub": {"type": "Property", "value": "urn:ngsi-ld:null"}}


*** Keywords ***
Create Entity With Attribute Expecting 400
    [Arguments]    ${attr_json}
    ${entity_id}=    Generate Random Building Entity Id
    ${payload}=    Evaluate
    ...    json.dumps({"id": "${entity_id}", "type": "Building", "probe": json.loads('''${attr_json}''')})
    ...    json
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    data=${payload}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    ${err_type}=    Evaluate    $response.json().get('type', '')
    Should Be Equal    ${err_type}    ${ERROR_TYPE_BAD_REQUEST_DATA}
