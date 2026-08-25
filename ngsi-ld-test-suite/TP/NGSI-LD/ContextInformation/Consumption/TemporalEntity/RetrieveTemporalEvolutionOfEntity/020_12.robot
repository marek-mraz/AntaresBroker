*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity with a LanguageProperty property

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Initial Temporal Entity
Test Template       Retrieve Temporal Entity


*** Variables ***
${vehicle_payload_file}=    vehicle-language-property-temporal-representation.jsonld


*** Test Cases ***    REPRESENTATION    EXPECTATION_FILENAME
020_12_01 Retrieve The Normalized Temporal Representation Of An Entity With A LanguageProperty Property
    [Tags]    te-retrieve    5_7_3    4_5_7    4_5_18    since_v1.4.1
    ${EMPTY}    vehicle-language-property-normalized-temporal-representation.jsonld
020_12_02 Retrieve The Simplified Temporal Representation Of An Entity With A LanguageProperty Property
    [Tags]    te-retrieve    5_7_3    4_5_9    4_5_18    since_v1.4.1
    temporalValues    vehicle-language-property-simplified-temporal-representation.jsonld


*** Keywords ***
Retrieve Temporal Entity
    [Documentation]    Check that one can retrieve the temporal evolution of an entity with a LanguageProperty property
    [Arguments]    ${representation}    ${expectation_filename}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    options=${representation}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    ${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ...    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
