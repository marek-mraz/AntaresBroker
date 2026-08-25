*** Settings ***
Documentation       Check that a BadRequestDataException is returned if the Retrieve Temporal Entity request is invalid with respect to pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Retrieve Temporal Entity With Invalid Pick Or Omit Query Params Usage


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    PICK    OMIT    ATTRS
020_22_01 RetrieveWithSameEntityMemberInPickAndOmit
    [Documentation]    Check that a BadRequestDataException is returned if an entity member is present in pick and omit
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    speed    speed    ${EMPTY}
020_22_02 RetrieveWithPickAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if pick and attrs query params are present
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    speed    ${EMPTY}    fuelLevel
02_22_03 RetrieveWithOmitAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if omit and attrs query params are present
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    ${EMPTY}    speed    fuelLevel


*** Keywords ***
Retrieve Temporal Entity With Invalid Pick Or Omit Query Params Usage
    [Documentation]    Check that a BadRequestDataException is returned if the Retrieve Temporal Entity request is invalid with respect to pick and omit query params
    [Arguments]    ${pick}    ${omit}    ${attrs}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    attrs=${attrs}
    ...    pick=${pick}
    ...    omit=${omit}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
