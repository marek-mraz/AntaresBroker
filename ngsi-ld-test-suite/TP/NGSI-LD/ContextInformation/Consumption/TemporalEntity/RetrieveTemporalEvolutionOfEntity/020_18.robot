*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property with temporal values

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Delete Temporal Attribute and Retrieve Temporal Evolution of Attribute


*** Variables ***
${vehicle_payload_file}=    vehicle-temporal-representation-different-attributes-types.jsonld


*** Test Cases ***    ATTR_NAME    SIMPLIFIED_TEMPORAL_KEY    EXPECTATION_FILENAME
020_18_01 With A Property
    [Tags]    te-retrieve    5_7_3    since_v1.6.1
    fuelLevel    values    vehicle-temporal-values-representation-property-020-18.jsonld
020_18_02 With A Relationship
    [Tags]    te-retrieve    5_7_3    since_v1.6.1
    isParkedIn    objects    vehicle-temporal-values-representation-relationship-020-18.jsonld
020_18_03 With A LanguageProperty
    [Tags]    te-retrieve    5_7_3    4_5_18    since_v1.6.1
    name    languageMaps    vehicle-temporal-values-representation-languageproperty-020-18.jsonld


*** Keywords ***
Delete Temporal Attribute and Retrieve Temporal Evolution of Attribute
    [Documentation]    Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property with temporal values
    [Arguments]    ${attr_name}    ${simplified_temporal_key}    ${expectation_filename}
    ${response}=    Delete Entity Attributes
    ...    entityId=${temporal_entity_representation_id}
    ...    attributeId=${attr_name}
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    ...    context=${ngsild_test_suite_context}

    ${response2}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    timeproperty=deletedAt
    ...    options=temporalValues
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response2.status_code}
    # Ignore the deletedAt value when checking the response body
    Check Response Body Containing EntityTemporal element
    ...    filename=${expectation_filename}
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    response_body=${response2.json()}
    ...    excluded_regex=root\\['${attr_name}'\\]\\['${simplified_temporal_key}'\\]\\[0\\]\\[1\\]
    # Check the deletedAt value is present in the response body
    Should Have Value In Json
    ...    json_object=${response2.json()}
    ...    json_path=['${attr_name}']['${simplified_temporal_key}'][0][1]

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
