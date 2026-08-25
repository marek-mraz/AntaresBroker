*** Settings ***
Documentation       Check that a ResourceNotFound error is returned if the temporal entity has no members left after applying pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Retrieve Temporal Entity With Pick And Omit Query Params


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    PICK    OMIT
020_23_01 RetrieveWithUnmatchedPick
    [Documentation]    Check that a ResourceNotFound error is returned if pick does not match any entity member
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    unknown    ${EMPTY}
020_23_02 RetrieveWithFullExcludingOmit
    [Documentation]    Check that a ResourceNotFound error is returned if omit contains all entity members
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    ${EMPTY}    id,type,fuelLevel,speed


*** Keywords ***
Retrieve Temporal Entity With Pick And Omit Query Params
    [Documentation]    Check that a ResourceNotFound error is returned if the entity has no members left after applying pick and omit query params
    [Arguments]    ${pick}    ${omit}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    pick=${pick}
    ...    omit=${omit}

    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
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
