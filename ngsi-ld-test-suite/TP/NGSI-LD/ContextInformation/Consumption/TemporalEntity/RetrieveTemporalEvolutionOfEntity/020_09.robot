*** Settings ***
Documentation       Check that one cannot retrieve the temporal evolution of an entity with an invalid request content

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Retrieve the temporal evolution of an entity with an invalid request content


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    TIMEREL    TIMEAT    ENDTIMEAT
020_09_01 After
    [Tags]    te-retrieve    5_7_3
    after    ${EMPTY}    ${EMPTY}
020_09_02 Before
    [Tags]    te-retrieve    5_7_3
    before    ${EMPTY}    ${EMPTY}
020_09_03 Between
    [Tags]    te-retrieve    5_7_3
    between    2020-08-01T12:00:00Z    ${EMPTY}


*** Keywords ***
Retrieve the temporal evolution of an entity with an invalid request content
    [Documentation]    Check that one cannot retrieve the temporal evolution of an entity with an invalid request content
    [Arguments]    ${timerel}    ${timeat}    ${endtimeat}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    timerel=${timerel}
    ...    timeAt=${timeat}
    ...    endTimeAt=${endtimeat}
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
    Set Test Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
