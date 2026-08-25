*** Settings ***
Documentation       Five brokers are set up A, B, C, D and E. A has two registrations, one auxiliary for the entity created in B, one inclusive for the entity created in C. B has two registrations, one redirect for the entity created in D and one redirect for the entity created in E. C shall establish one exclusive registration to E. Check that the entity returned from A has attributes from the entity in D and E.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${no_location_entity_payload_filename}               interoperability/offstreet-parking1-no-location.jsonld
${full_entity_payload_filename}                      interoperability/offstreet-parking1-full.jsonld
${inclusive_registration_payload_file_path}          csourceRegistrations/interoperability/context-source-registration-inclusive-1.jsonld
${auxiliary_registration_payload_file_path}          csourceRegistrations/interoperability/context-source-registration-auxiliary-2.jsonld
# exclusive-3 registers propertyNames [location] — the test asserts A gets E's location via C,
# which is impossible with exclusive-1 (registers only totalSpotsNumber; forwarded requests are
# narrowed to registered attrs per NGSI-LD 4.3.6.1).
${exclusive_registration_payload_file_path}          csourceRegistrations/interoperability/context-source-registration-exclusive-3.jsonld
${first_redirect_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-redirect-1.jsonld
${second_redirect_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-redirect-2.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}
${b5_url}

*** Test Cases ***
IOP_CNF_04_01 Retrieve OffStreetParking:1
    [Documentation]    Pre-conditions: no user context. Data only on leaves. D contains OffStreetParking1 without location. E contains OffStreetParking1.
    ...                Registrations established: Auxiliary in A to B and Inclusive in A to C. Redirect in B to D and Redirect in B to E. Exclusive in C to E.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    additive-auxiliary    proxy-redirect    proxy-exclusive    4_3_6    5_7_1

    #Client retrieves OffStreetParking:1 in A and checks for a successful response.
    ${response}=    Retrieve Entity    ${entity_id}    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${payload}=    Set To Dictionary    ${response.json()}

    #Client retrieves OffStreetParking:1 in D and E.
    ${response}=    Retrieve Entity    ${entity_id}    broker_url=${b4_url}    context=${ngsild_test_suite_context}
    ${first_expected_payload}=    Set To Dictionary    ${response.json()}
    ${response}=    Retrieve Entity    ${entity_id}    broker_url=${b5_url}    context=${ngsild_test_suite_context}
    ${second_expected_payload}=    Set To Dictionary    ${response.json()}

    #Client checks that the entity returned from A has attributes from the entity in D and E.
    Should Be Equal    ${payload}[availableSpotsNumber]    ${first_expected_payload}[availableSpotsNumber]
    Should Be Equal    ${payload}[totalSpotsNumber]    ${first_expected_payload}[totalSpotsNumber]
    Should Be Equal    ${payload}[location]    ${second_expected_payload}[location]

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity    ${no_location_entity_payload_filename}    ${entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${full_entity_payload_filename}    ${entity_id}    broker_url=${b5_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${auxiliary_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b2_url}
    ...    mode=auxiliary
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${first_redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b4_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${second_redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b5_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id4}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id4}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id4}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b3_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id5}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id5}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id5}
    ...    ${exclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b5_url}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}

    # Registrations propagate asynchronously to the broker's in-VM registry cache — an
    # immediate query/create can race ahead of the last registration (flaky aux/inclusive merges).
    Sleep    1s

Delete Entities And Delete Registrations
    Delete Context Source Registration    ${registration_id1}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id2}    broker_url=${b2_url}
    Delete Context Source Registration    ${registration_id3}    broker_url=${b2_url}
    Delete Context Source Registration    ${registration_id4}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id5}    broker_url=${b3_url}
    Delete Entity    ${entity_id}    broker_url=${b4_url}
    Delete Entity    ${entity_id}    broker_url=${b5_url}
